//! v0.54: behavioral test for the cross-context `CallerId` value (Q7).
//!
//! Drives the callee worker's `/_bynk/call/` dispatch with and without the
//! `X-Bynk-Caller` header: present → the `by c: Caller` handler reads the live
//! caller name; absent/empty → fail-closed (401, the `Internal`-channel
//! analogue). Skips loudly without a toolchain; `BYNK_REQUIRE_TSC=1` turns the
//! skip into a failure.

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
  on call by c: Caller (ping: String) -> Effect[Result[String, String]] {
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

const DRIVER_TS: &str = r#"import worker from "./workers/app-b/index.js";

function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error("FAIL: " + msg);
}

function call(headers: Record<string, string>): Request {
  return new Request("http://internal/_bynk/call/whoami", {
    method: "POST",
    headers: { "content-type": "application/json", ...headers },
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
    fs::write(run_dir.join("driver.ts"), DRIVER_TS).unwrap();
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
