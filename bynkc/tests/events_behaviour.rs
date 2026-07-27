//! Events track, slice 0 (spine #936) — behavioural (runtime) tests for the
//! emit/subscribe loop on the Bundle/node target.
//!
//! The e2e goldens and `tsc --strict` prove the emitted shape is *type-safe*;
//! neither proves a publisher's `Events.emit` actually *reaches* a subscriber's
//! `on event` handler at runtime, nor that release-at-commit (events.md §3.0)
//! really does suppress delivery when the emitting handler throws. This file
//! composes a real two-context project and runs it under node, observing the
//! subscriber's own side effect (a `Logger.info` call, captured via stdout)
//! as the only honest way to tell "was this actually delivered."
//!
//! Two scenarios in one driver:
//! - `markPaid`, a plain service, emits and completes normally — the
//!   subscriber must run exactly once.
//! - `bumpLedger` calls an agent (`Ledger`) whose handler emits *then* writes
//!   a `Cell` that violates a declared invariant — `commitState` throws before
//!   the flush is ever reached, so the subscriber must **not** run for that
//!   call. This is the edge the design explicitly promises (events.md §3.0,
//!   "an aborted handler emits nothing") and the one a golden diff or a type
//!   check cannot tell apart from a bug that flushes unconditionally.
//!
//! Skips loudly without `tsc`+`node`; `BYNK_REQUIRE_TSC=1` turns the skip into
//! a failure (CI), matching every other toolchain-driving test in this suite.

use bynkc::{BuildTarget, CompileOptions};
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

const REQUIRE_ENV: &str = "BYNK_REQUIRE_TSC";

fn base_command(program: &str) -> Command {
    if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(program);
        c
    } else {
        Command::new(program)
    }
}

fn tool_exists(name: &str) -> bool {
    let finder = if cfg!(windows) { "where" } else { "which" };
    Command::new(finder)
        .arg(name)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn discover_tsc() -> Option<(String, Vec<String>)> {
    if tool_exists("tsc") {
        return Some(("tsc".to_string(), vec![]));
    }
    if tool_exists("npx") {
        return Some((
            "npx".to_string(),
            vec![
                "--yes".to_string(),
                "-p".to_string(),
                "typescript@5".to_string(),
                "tsc".to_string(),
            ],
        ));
    }
    None
}

fn run(program: &str, prefix: &[String], args: &[&str], cwd: &Path) -> (bool, String) {
    let mut cmd = base_command(program);
    for p in prefix {
        cmd.arg(p);
    }
    for a in args {
        cmd.arg(a);
    }
    cmd.current_dir(cwd);
    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => return (false, format!("could not launch {program}: {e}")),
    };
    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&output.stderr));
    (output.status.success(), combined)
}

const TSCONFIG_JSON: &str = r#"{
  "compilerOptions": {
    "target": "ES2022",
    "module": "NodeNext",
    "moduleResolution": "NodeNext",
    "strict": true,
    "skipLibCheck": true,
    "outDir": "js",
    "rootDir": ".",
    "lib": ["ES2022", "DOM"]
  },
  "include": ["**/*.ts"]
}
"#;

// The publisher: a plain service (`markPaid`, the smallest shape from #939's
// own worked example) and an agent (`Ledger`) reached through a wrapping
// service (`bumpLedger`) — the transitive case, where the *handler* that
// emits is the agent's, not the caller's.
const SOURCE_ORDER: &str = r#"context commerce.order

exports transparent { PaymentConfirmed }
consumes bynk { Events }

event PaymentConfirmed = {
  orderId: String,
}

service markPaid {
  on call(orderId: String) -> Effect[()] given Events {
    Events.emit[PaymentConfirmed](PaymentConfirmed { orderId: orderId })
  }
}

agent Ledger {
  key id: String
  store total: Cell[Int] = 0

  invariant total_stays_small:
    total < 10

  on call bump(amount: Int) -> Effect[()] given Events {
    let _ <- total.update((n) => n + amount)
    do Events.emit[PaymentConfirmed](PaymentConfirmed { orderId: "ledger-event" })
  }
}

service bumpLedger {
  on call(id: String, amount: Int) -> Effect[()] {
    let ledger = Ledger(id)
    let _ <- ledger.bump(amount)
  }
}
"#;

// The subscriber: a pattern-less `from Events(E)` service (slice 0 — pattern
// refinement is slice 1) whose one observable effect is `Logger.info`, so the
// driver can tell delivery happened from the process's own stdout.
const SOURCE_NOTIFICATIONS: &str = r#"context commerce.notifications

consumes commerce.order
consumes bynk { Logger }

service OnPayment from Events(PaymentConfirmed) {
  on event(e: PaymentConfirmed) -> Effect[()] given Logger {
    Logger.info(e.orderId)
  }
}
"#;

const DRIVER_TS: &str = r#"import { composeApp } from "./compose.js";

function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error("FAIL: " + msg);
}

const app: any = composeApp();

// One emission from a plain service must reach the subscriber exactly once.
await app.order.markPaid("order-123");

// Same event type, emitted from an agent handler reached through a wrapping
// service — the transitive `__eventsDispatch` propagation case.
await app.order.bumpLedger("ledger-key", 3);

// This call violates the agent's invariant (0 + 3 + 15 = 18, not < 10):
// `commitState` throws before the flush is ever reached, so this must add
// no further delivery beyond the two above.
let threw = false;
try {
  await app.order.bumpLedger("ledger-key", 15);
} catch (e) {
  threw = true;
}
assert(threw, "the invariant-violating bump must throw");

console.log("ALL OK");
"#;

#[test]
fn bundle_events_publish_subscribe_and_abort_suppresses_delivery() {
    let runner = match discover_tsc() {
        Some(r) => r,
        None => {
            eprintln!("\n!!! EVENTS BEHAVIOUR VERIFICATION SKIPPED !!!\nno tsc runner.\n");
            if std::env::var(REQUIRE_ENV).is_ok() {
                panic!("{REQUIRE_ENV} is set but no tsc runner was found");
            }
            return;
        }
    };
    if !tool_exists("node") {
        eprintln!("\n!!! EVENTS BEHAVIOUR VERIFICATION SKIPPED !!!\n`node` not on PATH.\n");
        if std::env::var(REQUIRE_ENV).is_ok() {
            panic!("{REQUIRE_ENV} is set but `node` was not found");
        }
        return;
    }

    let tmp = std::env::temp_dir().join(format!("bynk-events-behaviour-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    let proj = tmp.join("proj/commerce");
    fs::create_dir_all(&proj).unwrap();
    fs::write(proj.join("order.bynk"), SOURCE_ORDER).unwrap();
    fs::write(proj.join("notifications.bynk"), SOURCE_NOTIFICATIONS).unwrap();

    let out = match bynkc::compile_project(
        &CompileOptions::single(tmp.join("proj")).target(BuildTarget::Bundle),
    ) {
        Ok(o) => o,
        Err(failure) => panic!(
            "compile the events project to a bundle:\n{}",
            bynkc::render_project_errors(&failure.flatten())
        ),
    };

    let run_dir = tmp.join("run");
    for file in &out.files {
        let target = run_dir.join(&file.output_path);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, &file.typescript).unwrap();
    }
    fs::write(
        run_dir.join("runtime.ts"),
        bynkc::emitter::emit_runtime_module(),
    )
    .unwrap();
    fs::write(run_dir.join("driver.ts"), DRIVER_TS).unwrap();
    fs::write(run_dir.join("tsconfig.json"), TSCONFIG_JSON).unwrap();
    fs::write(run_dir.join("package.json"), "{ \"type\": \"module\" }").unwrap();

    let (program, prefix) = &runner;
    let (ok, msg) = run(program, prefix, &["-p", "tsconfig.json"], &run_dir);
    assert!(ok, "tsc failed on the events-behaviour bundle:\n{msg}");

    let (ok, msg) = run("node", &[], &["js/driver.js"], &run_dir);
    assert!(
        ok && msg.contains("ALL OK"),
        "events-behaviour driver did not pass:\n{msg}"
    );

    // The subscriber's one observable effect (`Logger.info(e.orderId)`,
    // logged via `console.log`) must appear exactly twice: once for
    // `markPaid`'s emission, once for the *successful* `bumpLedger` call —
    // and must NOT appear a third time for the invariant-violating call,
    // which is the actual assertion this test exists to make.
    let delivered = msg.matches("order-123").count();
    assert_eq!(
        delivered, 1,
        "markPaid's emission must be delivered exactly once:\n{msg}"
    );
    let ledger_delivered = msg.matches("ledger-event").count();
    assert_eq!(
        ledger_delivered, 1,
        "exactly one of the two bumpLedger calls must deliver (the successful one, not the \
         invariant-violating one that aborts before the flush):\n{msg}"
    );

    let _ = fs::remove_dir_all(&tmp);
}
