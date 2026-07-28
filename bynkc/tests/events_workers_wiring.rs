//! Events track, slice 0 (spine #936, ADR 0284) — Workers-target wiring.
//!
//! `events_behaviour.rs` proves the emit/subscribe loop actually *runs* on
//! the Bundle/node target. Real delivery through a Cloudflare Durable Object
//! cannot be observed without `workerd`/`wrangler dev`, which this suite does
//! not have — so this file proves the two things that are checkable without
//! them: the emitted TypeScript type-checks under `tsc --strict` (nothing
//! here reads `wrangler.toml`, so a broken binding name would otherwise only
//! surface at deploy time), and the generated `wrangler.toml`/`events_fanout.
//! ts` actually declare the fan-out DO, its migration, and the
//! reverse-direction Service Binding a publisher needs to reach a subscriber
//! it does not itself `consumes` (the direction an ordinary `consumes` edge
//! never wires).

use bynkc::{BuildTarget, CompileOptions};
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

const REQUIRE_ENV: &str = "BYNK_REQUIRE_TSC";

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

const SOURCE_NOTIFICATIONS: &str = r#"context commerce.notifications

consumes commerce.order
consumes bynk { Logger }

service OnPayment from Events(PaymentConfirmed) {
  on event(e: PaymentConfirmed) -> Effect[()] given Logger {
    Logger.info(e.orderId)
  }
}
"#;

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
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(program);
        c
    } else {
        Command::new(program)
    };
    for p in prefix {
        cmd.arg(p);
    }
    for a in args {
        cmd.arg(a);
    }
    cmd.current_dir(cwd);
    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => return (false, format!("failed to spawn {program}: {e}")),
    };
    let mut msg = String::from_utf8_lossy(&output.stdout).into_owned();
    msg.push_str(&String::from_utf8_lossy(&output.stderr));
    (output.status.success(), msg)
}

#[test]
fn workers_events_fanout_do_and_wrangler_wiring() {
    let tmp =
        std::env::temp_dir().join(format!("bynk-events-workers-wiring-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    let proj = tmp.join("proj/commerce");
    fs::create_dir_all(&proj).unwrap();
    fs::write(proj.join("order.bynk"), SOURCE_ORDER).unwrap();
    fs::write(proj.join("notifications.bynk"), SOURCE_NOTIFICATIONS).unwrap();

    let out = match bynkc::compile_project(
        &CompileOptions::single(tmp.join("proj")).target(BuildTarget::Workers),
    ) {
        Ok(o) => o,
        Err(failure) => panic!(
            "compile the events project to workers:\n{}",
            bynkc::render_project_errors(&failure.flatten())
        ),
    };

    let find = |suffix: &str| -> String {
        out.files
            .iter()
            .find(|f| f.output_path.to_string_lossy().ends_with(suffix))
            .unwrap_or_else(|| panic!("no compiled file ends with {suffix:?}"))
            .typescript
            .clone()
    };

    // The publisher's wrangler.toml: the fan-out DO binding + migration, and
    // a Service Binding reaching the subscriber it does not itself `consumes`
    // (the reverse of the ordinary `consumes` edge — nothing else wires it).
    let order_wrangler = find("workers/commerce-order/wrangler.toml");
    assert!(
        order_wrangler.contains("name = \"___EVENTS_FANOUT\"")
            && order_wrangler.contains("class_name = \"__EventsFanout\""),
        "commerce-order's wrangler.toml must declare the fan-out DO binding:\n{order_wrangler}"
    );
    assert!(
        order_wrangler.contains("new_classes = [\"Ledger\", \"__EventsFanout\"]"),
        "the fan-out DO must ride the same migration as the context's real agents:\n{order_wrangler}"
    );
    assert!(
        order_wrangler.contains("binding = \"COMMERCE_NOTIFICATIONS\"")
            && order_wrangler.contains("service = \"commerce-notifications\""),
        "commerce-order must gain a reverse-direction Service Binding to its \
         subscriber (it does not itself `consumes` commerce.notifications):\n{order_wrangler}"
    );

    // The fan-out DO's own routing table: compile-time-known, not carried on
    // the wire (ADR 0284's DECISION — see `emit_events_fanout_do`).
    let fanout_ts = find("workers/commerce-order/events_fanout.ts");
    assert!(
        fanout_ts.contains("\"PaymentConfirmed\": [{ binding: \"COMMERCE_NOTIFICATIONS\", service: \"OnPayment\" }]"),
        "the fan-out DO's routing table must name the real subscriber:\n{fanout_ts}"
    );
    assert!(
        fanout_ts.contains("export class __EventsFanout"),
        "the fan-out DO class must be exported (wrangler resolves `class_name` \
         against the Worker's `main` exports):\n{fanout_ts}"
    );

    let order_index = find("workers/commerce-order/index.ts");
    assert!(
        order_index.contains("export { __EventsFanout } from \"./events_fanout.js\";"),
        "index.ts must re-export the fan-out DO class so wrangler's \
         `class_name` resolves against it:\n{order_index}"
    );

    // The subscriber's own wiring: the `/_bynk/event/` route the fan-out DO's
    // Service Binding call lands on, and no reciprocal fan-out DO of its own
    // (commerce.notifications never emits).
    let notif_index = find("workers/commerce-notifications/index.ts");
    assert!(
        notif_index.contains("/_bynk/event/") && notif_index.contains("case \"OnPayment\":"),
        "commerce-notifications's entry must route to its `on event` handler:\n{notif_index}"
    );
    let notif_wrangler = find("workers/commerce-notifications/wrangler.toml");
    assert!(
        !notif_wrangler.contains("EventsFanout"),
        "a context with no emitting handler must get no fan-out DO of its own:\n{notif_wrangler}"
    );

    // Real `tsc --strict` over the whole emitted tree — the actual emitter
    // gate (goldens only prove self-consistency); skips loudly without a
    // toolchain, `BYNK_REQUIRE_TSC=1` turns the skip into a failure (CI).
    let runner = match discover_tsc() {
        Some(r) => r,
        None => {
            eprintln!("\n!!! EVENTS WORKERS WIRING TSC CHECK SKIPPED !!!\nno tsc runner.\n");
            if std::env::var(REQUIRE_ENV).is_ok() {
                panic!("{REQUIRE_ENV} is set but no tsc runner was found");
            }
            let _ = fs::remove_dir_all(&tmp);
            return;
        }
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

    let (program, prefix) = &runner;
    let (ok, msg) = run(
        program,
        prefix,
        &["--strict", "--noEmit", "-p", "tsconfig.json"],
        &run_dir,
    );
    assert!(
        ok,
        "tsc --strict failed on the workers-target output:\n{msg}"
    );

    let _ = fs::remove_dir_all(&tmp);
}

// Events track, slice 1 (spine #936): the same fixture as above, but the
// subscriber's header carries a structural pattern. Deliver-and-filter (ADR
// 0286, unchanged by slice 1) means the fan-out DO's routing table and the
// wrangler.toml wiring are unaffected by a pattern — the guard lives inside
// the subscriber's own handler method, not in routing — so this asserts that
// negative directly rather than assuming it, alongside confirming the guard
// itself made it into the emitted TypeScript and still passes `tsc --strict`.
const SOURCE_NOTIFICATIONS_PATTERNED: &str = r#"context commerce.notifications

consumes commerce.order
consumes bynk { Logger }

service OnPayment from Events(PaymentConfirmed { orderId: "ledger-event", .. }) {
  on event(e: PaymentConfirmed) -> Effect[()] given Logger {
    Logger.info(e.orderId)
  }
}
"#;

#[test]
fn workers_events_pattern_leaves_fanout_routing_unchanged() {
    let tmp = std::env::temp_dir().join(format!(
        "bynk-events-workers-wiring-pattern-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&tmp);
    let proj = tmp.join("proj/commerce");
    fs::create_dir_all(&proj).unwrap();
    fs::write(proj.join("order.bynk"), SOURCE_ORDER).unwrap();
    fs::write(
        proj.join("notifications.bynk"),
        SOURCE_NOTIFICATIONS_PATTERNED,
    )
    .unwrap();

    let out = match bynkc::compile_project(
        &CompileOptions::single(tmp.join("proj")).target(BuildTarget::Workers),
    ) {
        Ok(o) => o,
        Err(failure) => panic!(
            "compile the patterned events project to workers:\n{}",
            bynkc::render_project_errors(&failure.flatten())
        ),
    };

    let find = |suffix: &str| -> String {
        out.files
            .iter()
            .find(|f| f.output_path.to_string_lossy().ends_with(suffix))
            .unwrap_or_else(|| panic!("no compiled file ends with {suffix:?}"))
            .typescript
            .clone()
    };

    // Byte-identical to the pattern-less fixture's own assertions: the
    // routing table and wrangler.toml wiring do not know a pattern exists.
    let order_wrangler = find("workers/commerce-order/wrangler.toml");
    assert!(
        order_wrangler.contains("name = \"___EVENTS_FANOUT\"")
            && order_wrangler.contains("class_name = \"__EventsFanout\"")
            && order_wrangler.contains("binding = \"COMMERCE_NOTIFICATIONS\""),
        "a subscription pattern must not change the fan-out DO/Service Binding wiring:\n{order_wrangler}"
    );
    let fanout_ts = find("workers/commerce-order/events_fanout.ts");
    assert!(
        fanout_ts.contains("\"PaymentConfirmed\": [{ binding: \"COMMERCE_NOTIFICATIONS\", service: \"OnPayment\" }]"),
        "a subscription pattern must not change the fan-out DO's routing table \
         (deliver-and-filter, ADR 0286 — the guard lives in the subscriber's own \
         handler, not in routing):\n{fanout_ts}"
    );

    // The guard itself must have made it into the subscriber's emitted
    // handler on the Workers target too (not just Bundle, per
    // `events_pattern_behaviour.rs`) — the generated handler method body
    // lands in `handlers.ts`, not the routing `index.ts`.
    let notif_handlers = find("workers/commerce-notifications/handlers.ts");
    assert!(
        notif_handlers.contains(r#"e.orderId === "ledger-event""#),
        "the pattern's guard must appear in the emitted handler:\n{notif_handlers}"
    );

    // Real `tsc --strict` over the whole emitted tree — the guard string is
    // hand-built (`event_pattern_guard`), so this is what actually catches a
    // syntactically-broken guard.
    let runner = match discover_tsc() {
        Some(r) => r,
        None => {
            eprintln!(
                "\n!!! EVENTS WORKERS WIRING (PATTERNED) TSC CHECK SKIPPED !!!\nno tsc runner.\n"
            );
            if std::env::var(REQUIRE_ENV).is_ok() {
                panic!("{REQUIRE_ENV} is set but no tsc runner was found");
            }
            let _ = fs::remove_dir_all(&tmp);
            return;
        }
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

    let (program, prefix) = &runner;
    let (ok, msg) = run(
        program,
        prefix,
        &["--strict", "--noEmit", "-p", "tsconfig.json"],
        &run_dir,
    );
    assert!(
        ok,
        "tsc --strict failed on the patterned workers-target output:\n{msg}"
    );

    let _ = fs::remove_dir_all(&tmp);
}
