//! #1758: `bynkc test` spawns each runner by the path detection resolved, and a
//! runner that is found but will not start is reported as such, not as missing.
//!
//! The Windows failure was a `tsc.cmd` shim that detection found (through
//! `PATHEXT`) but a bare-name spawn never started, so every runner was skipped
//! and the run advised installing `tsc`. The Windows test below pins the fix
//! where it can be seen: CI's Windows leg installs `typescript@5` with npm and
//! sets `BYNK_REQUIRE_TSC`. The Unix test pins the reporting half: a `tsc` on
//! `PATH` that is executable, so detection finds it, but whose interpreter does
//! not exist, so spawning it fails.

use std::path::{Path, PathBuf};
use std::process::Command;

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A one-commons project with one passing case.
fn one_case_project(project: &Path) {
    std::fs::create_dir_all(project.join("src/demo")).unwrap();
    std::fs::write(project.join("bynk.toml"), "[project]\nname = \"p\"\n").unwrap();
    std::fs::write(
        project.join("src/demo/m.bynk"),
        "commons demo.m\n\nfn one() -> Int { 1 }\n",
    )
    .unwrap();
    std::fs::write(
        project.join("src/demo/m_test.bynk"),
        "suite demo.m {\n  case \"one\" {\n    expect one() == 1\n  }\n}\n",
    )
    .unwrap();
}

/// #1758 acceptance: on Windows an npm-installed `tsc` is a `tsc.cmd` shim,
/// which `bynkc test` used to find but never start. Skipped unless
/// `BYNK_REQUIRE_TSC` is set (non-empty), as it is on CI.
#[cfg(windows)]
#[test]
fn an_npm_installed_tsc_shim_runs_the_suite() {
    if std::env::var_os("BYNK_REQUIRE_TSC").is_none_or(|v| v.is_empty()) {
        eprintln!("skipping: BYNK_REQUIRE_TSC is not set");
        return;
    }
    let project = scratch("runner-windows-shim").join("project");
    one_case_project(&project);
    let out = Command::new(env!("CARGO_BIN_EXE_bynkc"))
        .args(["test", "."])
        .current_dir(&project)
        .output()
        .expect("bynkc runs");
    assert!(
        out.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[cfg(unix)]
#[test]
fn a_runner_that_is_found_but_will_not_start_is_reported_not_called_missing() {
    use std::os::unix::fs::PermissionsExt;

    let dir = scratch("runner-will-not-start");
    let project = dir.join("project");
    one_case_project(&project);

    // The only thing on PATH: a `tsc` whose interpreter is missing.
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let tsc = bin.join("tsc");
    std::fs::write(&tsc, "#!/nonexistent/bynk-test-interpreter\n").unwrap();
    std::fs::set_permissions(&tsc, std::fs::Permissions::from_mode(0o755)).unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_bynkc"))
        .args(["test", "."])
        .current_dir(&project)
        .env("PATH", &bin)
        .output()
        .expect("bynkc runs");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "stderr:\n{stderr}");
    assert!(
        stderr.contains("no test runner could be started"),
        "a found runner that fails to start must be reported; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains(&format!(
            "`{}` was found but could not be started",
            tsc.display()
        )),
        "the report names the resolved path; stderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("requires either"),
        "an installed runner must not be reported as missing; stderr:\n{stderr}"
    );
}
