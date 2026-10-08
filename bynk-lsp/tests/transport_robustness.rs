//! #1667: the server binary survives a malformed client message, and its exit
//! code says whether the session ended in an orderly `shutdown`.
//!
//! Before #1667 a `didOpen` whose text held a lone UTF-16 surrogate escape
//! (`JSON.stringify` writes one for a lone surrogate in a JS string) ended the
//! server with exit 0 and nothing on stderr, and every ending exited 0.

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

fn frame(body: &str) -> Vec<u8> {
    format!("Content-Length: {}\r\n\r\n{body}", body.len()).into_bytes()
}

const INITIALIZE: &str =
    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#;
const INITIALIZED: &str = r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#;
const SHUTDOWN: &str = r#"{"jsonrpc":"2.0","id":2,"method":"shutdown"}"#;
const EXIT: &str = r#"{"jsonrpc":"2.0","method":"exit"}"#;

/// Run the server over `messages`, then close its input. Returns the exit
/// code, stdout and stderr.
fn session(tag: &str, messages: &[&str]) -> (i32, String, String) {
    // A private HOME, so the server's log goes to a scratch file.
    let home = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("lsp-{tag}"));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_bynkc-lsp"))
        .env("HOME", &home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the server starts");
    {
        let mut stdin = child.stdin.take().unwrap();
        for message in messages {
            // A write can fail once the server has gone; the exit code says why.
            if stdin.write_all(&frame(message)).is_err() {
                break;
            }
            let _ = stdin.flush();
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    // #1771 review: bounded, so a regression into a hang fails the test
    // rather than blocking CI. The pipes are read on threads so a full one
    // can't stall the child.
    let read = |pipe: Option<Box<dyn std::io::Read + Send>>| {
        std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_string(&mut text);
            }
            text
        })
    };
    let stdout = read(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
    );
    let stderr = read(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll the server") {
            break status;
        }
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{tag}: the server didn't exit within 30s of its input closing");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    (
        status.code().unwrap_or(-1),
        stdout.join().unwrap(),
        stderr.join().unwrap(),
    )
}

#[test]
fn a_lone_surrogate_in_a_message_does_not_end_the_session() {
    let did_open = r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///tmp/x.bynk","languageId":"bynk","version":1,"text":"a\udcffb"}}}"#;
    let (code, stdout, stderr) = session(
        "surrogate",
        &[INITIALIZE, INITIALIZED, did_open, SHUTDOWN, EXIT],
    );
    assert!(
        stdout.contains(r#""id":2"#),
        "the server must still answer `shutdown` after the bad message; stdout:\n{stdout}"
    );
    assert_eq!(code, 0, "an orderly shutdown exits 0; stderr:\n{stderr}");
}

#[test]
fn an_unparseable_message_is_skipped() {
    let (code, stdout, stderr) = session(
        "garbage",
        &[INITIALIZE, INITIALIZED, "{not json", SHUTDOWN, EXIT],
    );
    assert!(stdout.contains(r#""id":2"#), "stdout:\n{stdout}");
    assert_eq!(code, 0, "stderr:\n{stderr}");
}

#[test]
fn ending_without_shutdown_exits_non_zero_with_a_reason() {
    let (code, _stdout, stderr) = session("no-shutdown", &[INITIALIZE, INITIALIZED, EXIT]);
    assert_eq!(code, 1, "stderr:\n{stderr}");
    assert!(
        stderr.contains("without a `shutdown` request"),
        "stderr:\n{stderr}"
    );

    let (code, _stdout, stderr) = session("eof", &[INITIALIZE, INITIALIZED]);
    assert_eq!(
        code, 1,
        "closing input without `shutdown` is not orderly; stderr:\n{stderr}"
    );
}
