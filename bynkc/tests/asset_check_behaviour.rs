//! #1883, slice 1a (review of #1903): a source that declares an `asset`
//! checks clean through the real CLI, as a single file and as a project, and
//! only a build refuses it (`bynk.asset.not_yet_emitted`). Before the fix,
//! `bynkc check page.bynk` reused the single-file *build* and failed.
//!
//! Each case works in its own directory under `CARGO_TARGET_TMPDIR`: the CLI
//! may write a schema lock beside its input.

use std::path::{Path, PathBuf};
use std::process::Command;

const PAGE: &str = "commons compat.page\n\nasset style = \"assets/board.css\"\n\nfn sheet() -> String {\n  style.url\n}\n";

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn bynkc(args: &[&str], path: &Path) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_bynkc"))
        .args(args)
        .arg(path)
        .output()
        .expect("run bynkc");
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    s.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.success(), s)
}

#[test]
fn a_single_file_with_an_asset_checks_clean_and_its_build_is_refused() {
    let dir = scratch("asset-check-single");
    let file = dir.join("page.bynk");
    std::fs::write(&file, PAGE).unwrap();

    let (ok, out) = bynkc(&["check"], &file);
    assert!(ok, "`bynkc check` must pass, got:\n{out}");
    assert!(!out.contains("not_yet_emitted"), "{out}");

    let ts = dir.join("page.ts");
    let (ok, out) = bynkc(&["compile", "--output", ts.to_str().unwrap()], &file);
    assert!(!ok, "`bynkc compile` must refuse the asset, got:\n{out}");
    assert!(out.contains("bynk.asset.not_yet_emitted"), "{out}");
}

#[test]
fn a_project_with_an_asset_checks_clean() {
    let dir = scratch("asset-check-project");
    std::fs::create_dir_all(dir.join("src/compat")).unwrap();
    std::fs::write(dir.join("src/compat/page.bynk"), PAGE).unwrap();

    let (ok, out) = bynkc(&["check"], &dir);
    assert!(ok, "`bynkc check` on a project must pass, got:\n{out}");
}
