//! Behavioural test for the storage-track `store`/`Cell` emission (ADR 0109).
//!
//! Snapshots (the e2e golden) prove the emitted *shape*; this proves the
//! *runtime semantics* on the generated code — the same properties the ADR 0109
//! spike validated on a hand-written Durable Object, now on the emitter's output:
//!
//!   - a `:=` write persists across handler invocations;
//!   - a read after a `:=` in the same handler sees the written value
//!     (read-your-writes via the in-memory working state);
//!   - a handler whose committed state violates an invariant throws
//!     `InvariantViolation` and persists **nothing** (atomic revert — the gate
//!     runs before the durable write).
//!
//! It compiles a `Counter` agent in-process, then `tsc`-compiles the emitted
//! module + a driver and runs it under `node` against a fake Durable Object
//! storage. Like the tsc-verification stage it skips loudly when no TypeScript
//! toolchain is present; `BYNK_REQUIRE_TSC=1` turns the skip into a failure.

use std::fs;
use std::path::Path;
use std::process::Command;

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
        .output()
        .map(|o| o.status.success())
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
                "-y".to_string(),
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

const SOURCE: &str = "context shop\n\
\n\
agent Counter {\n\
\x20 key id: String\n\
\x20 store count: Cell[Int] = 0\n\
\n\
\x20 invariant nonneg: count >= 0\n\
\n\
\x20 on call set(n: Int) -> Effect[()] {\n\
\x20   count := n\n\
\x20   Effect.pure(())\n\
\x20 }\n\
\x20 on call get() -> Effect[Int] {\n\
\x20   count\n\
\x20 }\n\
\x20 on call setAndGet(n: Int) -> Effect[Int] {\n\
\x20   count := n\n\
\x20   count\n\
\x20 }\n\
}\n";

const DRIVER_TS: &str = r#"
import { Counter } from "./shop.js";

function assert(cond: boolean, msg: string): void {
  if (!cond) {
    throw new Error(`assertion failed: ${msg}`);
  }
}

// A fake Durable Object state: an in-memory key/value storage with the two
// methods the emitted agent uses.
function fakeState() {
  const m = new Map<string, unknown>();
  return {
    storage: {
      async get(key: string): Promise<unknown> {
        return m.get(key);
      },
      async put(key: string, value: unknown): Promise<void> {
        m.set(key, value);
      },
    },
  };
}

const c = new Counter(fakeState() as never);

// 1) A `:=` write persists, and is visible to a later handler.
await c.set(5, {});
assert((await c.get({})) === 5, "a `:=` write persists across handlers");

// 2) Read-your-writes: a read after `:=` in the same handler sees the value.
const r = await c.setAndGet(7, {});
assert(r === 7, "read-your-writes within a handler");
assert((await c.get({})) === 7, "the setAndGet write persisted");

// 3) Atomic revert: a commit that violates the invariant throws, and the
//    offending write does not persist.
let threw = false;
try {
  await c.set(-1, {});
} catch (e) {
  threw = String((e as { message?: string }).message ?? e).includes("InvariantViolation");
}
assert(threw, "an invariant-violating commit throws InvariantViolation");
assert((await c.get({})) === 7, "atomic revert: the violating write never persisted");

console.log("ALL OK");
"#;

const TSCONFIG_JSON: &str = r#"{
  "compilerOptions": {
    "module": "Node16",
    "moduleResolution": "node16",
    "target": "ES2022",
    "strict": true,
    "skipLibCheck": true,
    "outDir": "js",
    "rootDir": ".",
    "lib": ["ES2022", "DOM"]
  },
  "include": ["**/*.ts"],
  "exclude": ["js"]
}
"#;

#[test]
fn store_cell_agent_runtime_semantics() {
    let runner = match discover_tsc() {
        Some(r) => r,
        None => {
            eprintln!("\n!!! STORE-BEHAVIOUR VERIFICATION SKIPPED !!!\nno `tsc`/`npx` on PATH.\n");
            if std::env::var(REQUIRE_ENV).is_ok() {
                panic!("{REQUIRE_ENV} is set but no tsc runner was found");
            }
            return;
        }
    };
    if !tool_exists("node") {
        eprintln!("\n!!! STORE-BEHAVIOUR VERIFICATION SKIPPED !!!\n`node` is not on PATH.\n");
        if std::env::var(REQUIRE_ENV).is_ok() {
            panic!("{REQUIRE_ENV} is set but `node` was not found");
        }
        return;
    }

    // Compile the Counter agent in-process (bundle target).
    let tmp = std::env::temp_dir().join(format!("bynk-store-behaviour-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    let src = tmp.join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("shop.bynk"), SOURCE).unwrap();
    let out = bynkc::compile_project(&bynkc::CompileOptions::single(src.clone()))
        .map_err(bynkc::ProjectFailure::flatten)
        .expect("the Counter store agent must compile");

    for f in &out.files {
        let p = f.output_path.to_string_lossy();
        if p == "tsconfig.json" {
            continue;
        }
        let target_path = tmp.join(&f.output_path);
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&target_path, &f.typescript).unwrap();
    }
    fs::write(tmp.join("driver.ts"), DRIVER_TS).unwrap();
    fs::write(tmp.join("tsconfig.json"), TSCONFIG_JSON).unwrap();
    fs::write(tmp.join("package.json"), "{ \"type\": \"module\" }").unwrap();

    let (program, prefix) = &runner;
    let (ok, out_text) = run(program, prefix, &["-p", "tsconfig.json"], &tmp);
    assert!(ok, "tsc failed on the store-agent driver:\n{out_text}");

    let (ok, out_text) = run("node", &[], &["js/driver.js"], &tmp);
    let _ = fs::remove_dir_all(&tmp);
    assert!(
        ok && out_text.contains("ALL OK"),
        "store-agent behaviour driver did not pass:\n{out_text}"
    );
}
