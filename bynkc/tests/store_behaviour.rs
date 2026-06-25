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

const MAP_SOURCE: &str = "context shop\n\
\n\
agent Cart {\n\
\x20 key id: String\n\
\x20 store items: Map[String, Int]\n\
\n\
\x20 on call add(k: String, n: Int) -> Effect[()] {\n\
\x20   let _ <- items.put(k, n)\n\
\x20   Effect.pure(())\n\
\x20 }\n\
\x20 on call fetch(k: String) -> Effect[Option[Int]] {\n\
\x20   let r <- items.get(k)\n\
\x20   Effect.pure(r)\n\
\x20 }\n\
\x20 on call inc(k: String) -> Effect[()] {\n\
\x20   let _ <- items.upsert(k, 0, (x) => x + 1)\n\
\x20   Effect.pure(())\n\
\x20 }\n\
\x20 on call bumpStrict(k: String) -> Effect[()] {\n\
\x20   let _ <- items.update(k, (x) => x + 1)\n\
\x20   Effect.pure(())\n\
\x20 }\n\
\x20 on call drop(k: String) -> Effect[()] {\n\
\x20   let _ <- items.remove(k)\n\
\x20   Effect.pure(())\n\
\x20 }\n\
\x20 on call count() -> Effect[Int] {\n\
\x20   let n <- items.size()\n\
\x20   Effect.pure(n)\n\
\x20 }\n\
}\n";

const MAP_DRIVER_TS: &str = r#"
import { Cart } from "./shop.js";

function assert(cond: boolean, msg: string): void {
  if (!cond) {
    throw new Error(`assertion failed: ${msg}`);
  }
}

function fakeState() {
  const m = new Map<string, unknown>();
  return {
    storage: {
      async get(key: string): Promise<unknown> { return m.get(key); },
      async put(key: string, value: unknown): Promise<void> { m.set(key, value); },
    },
  };
}

const c = new Cart(fakeState() as never);

// put + get
await c.add("a", 5, {});
let r = await c.fetch("a", {});
assert(r.tag === "Some" && r.value === 5, "put then get");

// upsert on existing, then default-if-absent
await c.inc("a", {});
r = await c.fetch("a", {});
assert(r.tag === "Some" && r.value === 6, "upsert on an existing key");
await c.inc("b", {});
r = await c.fetch("b", {});
assert(r.tag === "Some" && r.value === 1, "upsert default-if-absent");

assert((await c.count({})) === 2, "size counts entries");

// remove
await c.drop("a", {});
r = await c.fetch("a", {});
assert(r.tag === "None", "remove deletes the entry");
assert((await c.count({})) === 1, "size after remove");

// update on an absent key faults — and nothing commits (atomic revert)
let threw = false;
try {
  await c.bumpStrict("missing", {});
} catch (e) {
  threw = String((e as { message?: string }).message ?? e).includes("Map.update: key absent");
}
assert(threw, "update on an absent key throws");
assert((await c.count({})) === 1, "atomic revert: the faulting update left the map unchanged");

console.log("ALL OK");
"#;

const SET_SOURCE: &str = "context shop\n\
\n\
agent Tags {\n\
\x20 key id: String\n\
\x20 store tags: Set[String]\n\
\n\
\x20 on call add(t: String) -> Effect[()] {\n\
\x20   let _ <- tags.add(t)\n\
\x20   Effect.pure(())\n\
\x20 }\n\
\x20 on call drop(t: String) -> Effect[()] {\n\
\x20   let _ <- tags.remove(t)\n\
\x20   Effect.pure(())\n\
\x20 }\n\
\x20 on call has(t: String) -> Effect[Bool] {\n\
\x20   let r <- tags.contains(t)\n\
\x20   Effect.pure(r)\n\
\x20 }\n\
\x20 on call count() -> Effect[Int] {\n\
\x20   let n <- tags.size()\n\
\x20   Effect.pure(n)\n\
\x20 }\n\
}\n";

const SET_DRIVER_TS: &str = r#"
import { Tags } from "./shop.js";

function assert(cond: boolean, msg: string): void {
  if (!cond) {
    throw new Error(`assertion failed: ${msg}`);
  }
}

function fakeState() {
  const m = new Map<string, unknown>();
  return {
    storage: {
      async get(key: string): Promise<unknown> { return m.get(key); },
      async put(key: string, value: unknown): Promise<void> { m.set(key, value); },
    },
  };
}

const c = new Tags(fakeState() as never);

// add + contains
assert((await c.has("x", {})) === false, "absent before add");
await c.add("x", {});
assert((await c.has("x", {})) === true, "contains after add");

// idempotent add
await c.add("x", {});
assert((await c.count({})) === 1, "add is idempotent");

await c.add("y", {});
assert((await c.count({})) === 2, "size counts members");

// remove
await c.drop("x", {});
assert((await c.has("x", {})) === false, "remove deletes the member");
assert((await c.count({})) === 1, "size after remove");

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

/// Compile `source` (a one-file `context shop` project, bundle target), write it
/// alongside `driver`, then `tsc`-compile and run the driver under node, asserting
/// it prints `ALL OK`. Skips loudly without a TS toolchain.
fn verify(tag: &str, source: &str, driver: &str) {
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

    let tmp =
        std::env::temp_dir().join(format!("bynk-store-behaviour-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    let src = tmp.join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("shop.bynk"), source).unwrap();
    let out = bynkc::compile_project(&bynkc::CompileOptions::single(src.clone()))
        .map_err(bynkc::ProjectFailure::flatten)
        .expect("the store agent must compile");

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
    fs::write(tmp.join("driver.ts"), driver).unwrap();
    fs::write(tmp.join("tsconfig.json"), TSCONFIG_JSON).unwrap();
    fs::write(tmp.join("package.json"), "{ \"type\": \"module\" }").unwrap();

    let (program, prefix) = &runner;
    let (ok, out_text) = run(program, prefix, &["-p", "tsconfig.json"], &tmp);
    assert!(
        ok,
        "tsc failed on the store-agent driver ({tag}):\n{out_text}"
    );

    let (ok, out_text) = run("node", &[], &["js/driver.js"], &tmp);
    let _ = fs::remove_dir_all(&tmp);
    assert!(
        ok && out_text.contains("ALL OK"),
        "store-agent behaviour driver ({tag}) did not pass:\n{out_text}"
    );
}

#[test]
fn store_cell_agent_runtime_semantics() {
    verify("cell", SOURCE, DRIVER_TS);
}

#[test]
fn store_map_agent_runtime_semantics() {
    verify("map", MAP_SOURCE, MAP_DRIVER_TS);
}

#[test]
fn store_set_agent_runtime_semantics() {
    verify("set", SET_SOURCE, SET_DRIVER_TS);
}
