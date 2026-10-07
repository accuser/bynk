//! #1666: `bynkc check` on a file with a megabyte line. The report used to
//! cost time and output in proportion to the line (seconds, and five ANSI
//! escapes a character, even with stderr piped to a file); now the line is cut
//! to a window around the label, and colour is off when stderr isn't a
//! terminal.

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

#[test]
fn a_megabyte_line_renders_small_fast_and_uncoloured_when_piped() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("render-long-lines");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("long.bynk");
    std::fs::write(&file, format!("commons c\n{}\n", "a".repeat(1_000_000))).unwrap();

    let started = Instant::now();
    let out = Command::new(env!("CARGO_BIN_EXE_bynkc"))
        .arg("check")
        .arg(&file)
        .env_remove("NO_COLOR")
        .output()
        .expect("bynkc runs");
    let took = started.elapsed();
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(!out.status.success(), "the file has a parse error");
    assert!(
        stderr.contains("long.bynk:2:1 "),
        "the report names the line:\n{stderr}"
    );
    assert!(
        !stderr.contains('\u{1b}'),
        "stderr is a pipe here, so no colour:\n{stderr}"
    );
    assert!(
        stderr.len() < 8_000,
        "the report is cut to a window, not the whole line: {} bytes",
        stderr.len()
    );
    // Generous for a debug build on a loaded runner; before #1666 a release
    // build took seconds.
    assert!(took < Duration::from_secs(20), "took {took:?}");
}
