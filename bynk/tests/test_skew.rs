//! #1675: `bynk test` acts on the driver↔compiler skew it computes, end to end
//! — a real `bynk test` run against a fake `bynkc` (via `BYNK_BYNKC`) that
//! reports a chosen version and echoes its arguments when it is run.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};

/// A fake `bynkc` reporting `version` for `--version`, and printing a marker
/// (proof it was delegated to) for anything else.
fn fake_bynkc(name: &str, version: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("test-skew")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bynkc");
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo \"bynkc {version}\"; exit 0; fi\necho \"FAKE BYNKC RAN: $*\"\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn bynk_test(bynkc: &PathBuf, extra: &[&str], allow_env: bool) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_bynk"));
    cmd.arg("test")
        .arg(".")
        .args(extra)
        .env("BYNK_BYNKC", bynkc);
    if allow_env {
        cmd.env("BYNK_ALLOW_SKEW", "1");
    } else {
        cmd.env_remove("BYNK_ALLOW_SKEW");
    }
    cmd.output().expect("run bynk test")
}

/// The driver's own version, as `MAJOR.MINOR.PATCH`.
fn driver() -> (u32, u32) {
    let v = env!("CARGO_PKG_VERSION");
    let mut it = v.split('.').map(|p| p.parse::<u32>().unwrap());
    (it.next().unwrap(), it.next().unwrap())
}

#[test]
fn minor_skew_warns_and_still_runs() {
    let (major, minor) = driver();
    let bynkc = fake_bynkc("minor", &format!("{major}.{}.0", minor + 1));
    let out = bynk_test(&bynkc, &[], false);
    let (stdout, stderr) = (
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    assert!(
        stderr.contains("different minor version"),
        "warns: {stderr}"
    );
    assert!(
        stdout.contains("FAKE BYNKC RAN: test"),
        "delegates: {stdout}"
    );
    assert!(out.status.success(), "a minor skew does not fail the run");
}

#[test]
fn major_skew_refuses_without_running() {
    let (major, _) = driver();
    let bynkc = fake_bynkc("major", &format!("{}.0.0", major + 1));
    let out = bynk_test(&bynkc, &[], false);
    let (stdout, stderr) = (
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    assert!(!out.status.success(), "a major skew fails");
    assert!(
        stderr.contains("different major version") && stderr.contains("Refusing"),
        "explains: {stderr}"
    );
    assert!(
        !stdout.contains("FAKE BYNKC RAN"),
        "never delegates: {stdout}"
    );
}

#[test]
fn major_skew_runs_when_allowed() {
    let (major, _) = driver();
    let bynkc = fake_bynkc("major-allowed", &format!("{}.0.0", major + 1));
    for (extra, env) in [(&["--allow-skew"][..], false), (&[][..], true)] {
        let out = bynk_test(&bynkc, extra, env);
        let (stdout, stderr) = (
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
        assert!(stderr.contains("running it anyway"), "warns: {stderr}");
        assert!(
            stdout.contains("FAKE BYNKC RAN: test"),
            "delegates: {stdout}"
        );
    }
}
