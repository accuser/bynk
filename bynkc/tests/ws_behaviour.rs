//! v0.103 (real-time track slice 3a): behavioural proof that a `from WebSocket`
//! service *runs* on the bundle target against `TestConnection`. Compiles the
//! §20 chat-room fixture in-process, then a Node driver drives the `on open`
//! handler with a `TestConnection` and asserts the held connection flowed
//! through — the welcome frame the handler sent is captured on the connection
//! before it is transferred into the Room agent.
//!
//! Like the tsc-verification stage, this skips loudly when no TypeScript
//! toolchain is available; `BYNK_REQUIRE_TSC=1` turns the skip into a failure.

use std::fs;
use std::path::{Path, PathBuf};
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
    base_command(finder)
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

const DRIVER_TS: &str = r#"
import { ChatGateway, RoomId, UserId } from "./chat.js";
import { TestConnection } from "./runtime.js";

function assert(cond: boolean, msg: string): void {
  if (!cond) {
    throw new Error(`assertion failed: ${msg}`);
  }
}

async function main(): Promise<void> {
  // The bundle realisation of a held connection: capture-and-inspect.
  const tc = new TestConnection<{ text: string }>();
  const roomId = RoomId.unsafe("room-1");
  const user = UserId.unsafe("alice");

  // Drive the WebSocket `on open` handler directly with the TestConnection —
  // exactly what the Workers upgrade does after authenticating the actor.
  await ChatGateway.open(tc, roomId, { identity: user });

  // The held connection flowed through `on open`: the welcome frame was sent on
  // it (captured by the TestConnection) before it was transferred into the agent.
  assert(tc.sent.length === 1, `one frame sent on the connection, got ${tc.sent.length}`);
  assert(tc.sent[0].text === "welcome", `the welcome frame was captured, got ${JSON.stringify(tc.sent[0])}`);
  // `send` is non-consuming; `close` was never called — the connection was
  // transferred to the agent, not disposed at the edge.
  assert(tc.closed === false, "the connection is transferred (open), not closed");

  // Slice 3b-iii — inbound: drive `on message` with a decoded frame and the firing
  // (borrowed) connection. The handler echoes the frame's text back on it.
  await ChatGateway.message(tc, { text: "hello" }, { identity: user });
  assert(tc.sent.length === 2, `the inbound echo was sent, got ${tc.sent.length} frames`);
  assert(tc.sent[1].text === "hello", `the echo carried the inbound text, got ${JSON.stringify(tc.sent[1])}`);
  // The connection is borrowed in `on message` — used (`send`), never disposed.
  assert(tc.closed === false, "the borrowed connection stays open after `on message`");

  // Slice 3b-iii — close: drive `on close`; it disposes the stored connection via
  // the agent. The handler ran without error (no send/close on the firing socket).
  await ChatGateway.close(tc, roomId, { identity: user });
  assert(tc.sent.length === 2, "`on close` sends no frame on the firing connection");

  console.log("ALL OK");
}

main().catch((e: unknown) => {
  console.error(e);
  // Re-throw so an assertion failure surfaces as a non-zero exit (and no
  // "ALL OK" is printed).
  throw e;
});
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
fn websocket_chatroom_runs_on_bundle() {
    let runner = match discover_tsc() {
        Some(r) => r,
        None => {
            eprintln!(
                "\n!!! WEBSOCKET BEHAVIOUR VERIFICATION SKIPPED !!!\nno tsc runner on PATH.\n"
            );
            if std::env::var(REQUIRE_ENV).is_ok() {
                panic!("{REQUIRE_ENV} is set but no tsc runner was found");
            }
            return;
        }
    };
    if !tool_exists("node") {
        eprintln!("\n!!! WEBSOCKET BEHAVIOUR VERIFICATION SKIPPED !!!\n`node` is not on PATH.\n");
        if std::env::var(REQUIRE_ENV).is_ok() {
            panic!("{REQUIRE_ENV} is set but `node` was not found");
        }
        return;
    }

    // Compile the chat-room fixture (bundle) in-process.
    let fixture: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/positive/236_websocket_chatroom/src");
    let out = bynkc::compile_project(
        &bynkc::CompileOptions::single(fixture).target(bynkc::BuildTarget::Bundle),
    )
    .map_err(bynkc::ProjectFailure::flatten)
    .expect("the chat-room bundle fixture must compile");

    let tmp = std::env::temp_dir().join(format!("bynk-ws-behaviour-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();
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

    // Type-check and compile to JS, then run the driver under Node.
    let (ok, log) = run(&runner.0, &runner.1, &["--project", "tsconfig.json"], &tmp);
    assert!(ok, "the chat-room driver must type-check + compile:\n{log}");
    let (ran, log) = run("node", &[], &["js/driver.js"], &tmp);
    assert!(
        ran && log.contains("ALL OK"),
        "the chat-room driver must run green:\n{log}"
    );

    let _ = fs::remove_dir_all(&tmp);
}
