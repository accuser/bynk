//! v0.54: behavioral tests for the cross-context `CallerId` value (Q7).
//!
//! Workers (`cross_context_caller_reads_live_id_and_fails_closed`): drives the
//! callee worker's `/_bynk/call/` dispatch with and without the `X-Bynk-Caller`
//! header: present → the `by c: Caller` handler reads the live caller name;
//! absent/empty → fail-closed (401, the `Internal`-channel analogue).
//!
//! Bundle (`bundle_cross_context_caller_reads_the_consuming_context_name`,
//! v0.54 / #655): the same handler on the bundle target, where a cross-context
//! call is a direct `composeApp`-wired invocation. Composes the app and checks
//! each consumer reads its own name and the top-level entry self-attributes —
//! the runtime half the `396_…` fixture only tsc-checks.
//!
//! Both skip loudly without a toolchain; `BYNK_REQUIRE_TSC=1` turns the skip
//! into a failure.

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

const SOURCE_B: &str = r#"context app.b

service whoami {
  on call(ping: String) -> Effect[Result[String, String]] by c: Caller {
    Ok(c.identity)
  }
}
"#;

const SOURCE_A: &str = r#"context app.a

consumes app.b as B

service ask {
  on call(ping: String) -> Effect[Result[String, String]] {
    let r <- B.whoami(ping)
    r
  }
}
"#;

// A second consumer of the same provider, to prove each consumer's cross-context
// call reads *its own* qualified name — the per-consumer surface the compose root
// builds, not one shared instance carrying a single caller.
const SOURCE_D: &str = r#"context app.d

consumes app.b as B

service probe {
  on call(ping: String) -> Effect[Result[String, String]] {
    let r <- B.whoami(ping)
    r
  }
}
"#;

// v0.54 (#655): the bundle-mode analogue of the workers driver below. There is no
// `/_bynk/call/` door and no header — cross-context calls are direct invocations
// wired by `composeApp`, so the driver composes the app and reads the caller each
// path threads through `makeSurface`.
const BUNDLE_DRIVER_TS: &str = r#"import { composeApp } from "./compose.js";

function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error("FAIL: " + msg);
}

const app: any = composeApp();

// A consumer's cross-context call reads the *consumer's* qualified name.
let r: any = await app.a.ask("hi");
assert(r.tag === "Ok" && r.value === "app.a", "app.a reads its own name, got " + JSON.stringify(r));

// A second consumer of the same provider reads *its* name — proving the surface
// is built per-consumer, not shared with a single baked-in caller.
r = await app.d.probe("hi");
assert(r.tag === "Ok" && r.value === "app.d", "app.d reads its own name, got " + JSON.stringify(r));

// The top-level entry addresses the provider directly: no calling context, so it
// self-attributes with the provider's own qualified name (ADR 0092 note — this is
// where bundle and workers diverge; workers' internal door fail-closes instead).
r = await app.b.whoami("hi");
assert(r.tag === "Ok" && r.value === "app.b", "top-level self-attributes, got " + JSON.stringify(r));

console.log("ALL OK");
"#;

const DRIVER_TS: &str = r#"import worker from "./workers/app-b/index.js";

// v0.177 (#643): the compiled contract hash for `whoami`, extracted from the
// emitted callee and injected here. A correct caller stamps it beside the
// caller identity.
const CONTRACT = "__CONTRACT_HASH__";

function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error("FAIL: " + msg);
}

function call(headers: Record<string, string>): Request {
  return new Request("http://internal/_bynk/call/whoami", {
    method: "POST",
    // A well-formed Bynk call stamps the contract hash; individual cases
    // override it to drive the skew paths.
    headers: { "content-type": "application/json", "X-Bynk-Contract": CONTRACT, ...headers },
    body: JSON.stringify("hello"),
  });
}

const env: any = {};

// 1. With the caller header → the body reads the live caller name.
let res = await worker.fetch(call({ "X-Bynk-Caller": "app.a" }), env);
assert(res.status === 200, "with caller header → 200, got " + res.status);
let body: any = await res.json();
assert(body.kind === "Ok" && body.value === "app.a", "body returns the caller id");

// 2. No caller header → fail-closed (401).
res = await worker.fetch(call({}), env);
assert(res.status === 401, "absent caller header → 401, got " + res.status);

// 3. Empty caller header → fail-closed (401).
res = await worker.fetch(call({ "X-Bynk-Caller": "" }), env);
assert(res.status === 401, "empty caller header → 401, got " + res.status);

// v0.177 (#643): the deploy-skew seam.
//
// 4. A caller compiled against a *different* contract → fail closed with 409 and
//    a named ContractMismatch. This is the whole point: the pair disagree, so
//    the payload's interpretation is in doubt and the callee refuses rather than
//    misreading it.
res = await worker.fetch(
  call({ "X-Bynk-Caller": "app.a", "X-Bynk-Contract": "0000000000000000" }),
  env,
);
assert(res.status === 409, "skewed contract → 409, got " + res.status);
body = await res.json();
assert(body.kind === "ContractMismatch", "409 body names the fault");
assert(body.service === "whoami", "409 body names the service");
assert(body.expected === CONTRACT, "409 body reports the expected hash");
assert(body.actual === "0000000000000000", "409 body reports what arrived");

// 5. Absent contract header → fail closed too. A Bynk caller always stamps one,
//    so its absence is a non-Bynk or pre-upgrade caller — skewed by definition.
res = await worker.fetch(call({ "X-Bynk-Caller": "app.a", "X-Bynk-Contract": "" }), env);
assert(res.status === 409, "empty contract header → 409, got " + res.status);

// 6. The contract check precedes the caller check: with contracts in doubt,
//    nothing about the request is trustworthy, including the identity.
res = await worker.fetch(call({ "X-Bynk-Contract": "0000000000000000" }), env);
assert(res.status === 409, "skewed contract outranks a missing caller, got " + res.status);

console.log("ALL OK");
"#;

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

#[test]
fn cross_context_caller_reads_live_id_and_fails_closed() {
    let runner = match discover_tsc() {
        Some(r) => r,
        None => {
            eprintln!("\n!!! CROSS-CONTEXT CALLER VERIFICATION SKIPPED !!!\nno tsc runner.\n");
            if std::env::var(REQUIRE_ENV).is_ok() {
                panic!("{REQUIRE_ENV} is set but no tsc runner was found");
            }
            return;
        }
    };
    if !tool_exists("node") {
        eprintln!("\n!!! CROSS-CONTEXT CALLER VERIFICATION SKIPPED !!!\n`node` not on PATH.\n");
        if std::env::var(REQUIRE_ENV).is_ok() {
            panic!("{REQUIRE_ENV} is set but `node` was not found");
        }
        return;
    }

    let tmp = std::env::temp_dir().join(format!("bynk-xctx-caller-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    let proj = tmp.join("proj/app");
    fs::create_dir_all(&proj).unwrap();
    fs::write(proj.join("a.bynk"), SOURCE_A).unwrap();
    fs::write(proj.join("b.bynk"), SOURCE_B).unwrap();

    let out = match bynkc::compile_project(
        &CompileOptions::single(tmp.join("proj")).target(BuildTarget::Workers),
    ) {
        Ok(o) => o,
        Err(failure) => panic!(
            "compile the cross-context project to Workers:\n{}",
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
    // v0.177 (#643): read the callee's compiled constant out of its emitted
    // entry and give it to the driver, so the guard drives the *real* hash
    // rather than restating one (which would only prove the test agrees with
    // itself).
    let callee = out
        .files
        .iter()
        .find(|f| f.output_path.ends_with("workers/app-b/index.ts"))
        .expect("app-b entry emitted");
    let hash = regex::Regex::new(r#"expected: "([0-9a-f]{16})""#)
        .unwrap()
        .captures(&callee.typescript)
        .map(|c| c[1].to_string())
        .expect("app-b's entry stamps a contract hash");
    fs::write(
        run_dir.join("driver.ts"),
        DRIVER_TS.replace("__CONTRACT_HASH__", &hash),
    )
    .unwrap();
    fs::write(run_dir.join("tsconfig.json"), TSCONFIG_JSON).unwrap();
    fs::write(run_dir.join("package.json"), "{ \"type\": \"module\" }").unwrap();

    let (program, prefix) = &runner;
    let (ok, msg) = run(program, prefix, &["-p", "tsconfig.json"], &run_dir);
    assert!(ok, "tsc failed on the cross-context-caller workers:\n{msg}");

    let (ok, msg) = run("node", &[], &["js/driver.js"], &run_dir);
    assert!(
        ok && msg.contains("ALL OK"),
        "cross-context-caller driver did not pass:\n{msg}"
    );
    let _ = fs::remove_dir_all(&tmp);
}

/// v0.54 (#655): the bundle-target twin of the workers test above. A bundle-mode
/// `by c: Caller` handler could not even compile before this fix (its
/// `makeSurface` fed `deps` without the `identity` field it typed, TS2345). This
/// drives the *runtime* value the fixture (`396_service_caller_binder_bundle_surface`)
/// only tsc-checks: each consumer reads its own qualified name, and the top-level
/// entry self-attributes — including the multiple-consumer path the compose root's
/// per-consumer surfaces exist to serve.
#[test]
fn bundle_cross_context_caller_reads_the_consuming_context_name() {
    let runner = match discover_tsc() {
        Some(r) => r,
        None => {
            eprintln!(
                "\n!!! BUNDLE CROSS-CONTEXT CALLER VERIFICATION SKIPPED !!!\nno tsc runner.\n"
            );
            if std::env::var(REQUIRE_ENV).is_ok() {
                panic!("{REQUIRE_ENV} is set but no tsc runner was found");
            }
            return;
        }
    };
    if !tool_exists("node") {
        eprintln!(
            "\n!!! BUNDLE CROSS-CONTEXT CALLER VERIFICATION SKIPPED !!!\n`node` not on PATH.\n"
        );
        if std::env::var(REQUIRE_ENV).is_ok() {
            panic!("{REQUIRE_ENV} is set but `node` was not found");
        }
        return;
    }

    let tmp = std::env::temp_dir().join(format!("bynk-xctx-caller-bundle-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    let proj = tmp.join("proj/app");
    fs::create_dir_all(&proj).unwrap();
    fs::write(proj.join("a.bynk"), SOURCE_A).unwrap();
    fs::write(proj.join("b.bynk"), SOURCE_B).unwrap();
    fs::write(proj.join("d.bynk"), SOURCE_D).unwrap();

    let out = match bynkc::compile_project(
        &CompileOptions::single(tmp.join("proj")).target(BuildTarget::Bundle),
    ) {
        Ok(o) => o,
        Err(failure) => panic!(
            "compile the cross-context project to a bundle:\n{}",
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
    fs::write(run_dir.join("driver.ts"), BUNDLE_DRIVER_TS).unwrap();
    fs::write(run_dir.join("tsconfig.json"), TSCONFIG_JSON).unwrap();
    fs::write(run_dir.join("package.json"), "{ \"type\": \"module\" }").unwrap();

    let (program, prefix) = &runner;
    let (ok, msg) = run(program, prefix, &["-p", "tsconfig.json"], &run_dir);
    assert!(ok, "tsc failed on the cross-context-caller bundle:\n{msg}");

    let (ok, msg) = run("node", &[], &["js/driver.js"], &run_dir);
    assert!(
        ok && msg.contains("ALL OK"),
        "bundle cross-context-caller driver did not pass:\n{msg}"
    );
    let _ = fs::remove_dir_all(&tmp);
}
