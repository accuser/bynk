//! #1663: checking now continues past a resolve error and past a syntax error
//! (a recovering parse), so partial programs flow further than they did. None
//! of that may reach emission: a program with any error-severity diagnostic
//! still produces no output, from `compile`, `compile_project`, or
//! `bynkc test`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const RESOLVE_ERROR: &str = "commons demo\n\nfn a() -> Int { 1 }\n\nfn b() -> Int { nope }\n";
const SYNTAX_ERROR: &str = "commons demo\n\nfn a() -> Int { 1 +  }\n\nfn b() -> Int { 2 }\n";
const CLEAN: &str = "commons demo\n\nfn a() -> Int { 1 }\n";
const SUITE: &str = "suite demo\n\ncase \"a is one\" {\n  expect a() == 1\n}\n";

fn project(name: &str, source: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("recovery-never-emits")
        .join(name);
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(
        root.join("bynk.toml"),
        "[project]\nname = \"recovery\"\nversion = \"0.1.0\"\n\n[paths]\ninclude = [\"src\", \"tests\"]\n",
    )
    .unwrap();
    fs::write(root.join("src/demo.bynk"), source).unwrap();
    fs::write(root.join("tests/demo.bynk"), SUITE).unwrap();
    root
}

fn compiles(root: &Path) -> bool {
    let paths = bynkc::try_read_project_paths(root).expect("well-formed manifest");
    bynkc::compile_project(&bynk_testkit::compile_options_split(
        root.to_path_buf(),
        paths,
    ))
    .is_ok()
}

#[test]
fn single_file_compile_refuses_a_partial_program() {
    assert!(bynkc::compile(RESOLVE_ERROR, "demo.bynk").is_err());
    assert!(bynkc::compile(SYNTAX_ERROR, "demo.bynk").is_err());
}

#[test]
fn project_compile_refuses_a_partial_program() {
    // The control: the same project without the error compiles, so a refusal
    // below is the error's doing, not the harness's.
    assert!(compiles(&project("clean", CLEAN)));
    assert!(!compiles(&project("resolve", RESOLVE_ERROR)));
    assert!(!compiles(&project("syntax", SYNTAX_ERROR)));
}

#[test]
fn bynkc_test_runs_nothing_for_a_partial_program() {
    for (name, source) in [
        ("test-resolve", RESOLVE_ERROR),
        ("test-syntax", SYNTAX_ERROR),
    ] {
        let root = project(name, source);
        let out_dir = root.join("out");
        let out = Command::new(env!("CARGO_BIN_EXE_bynkc"))
            .arg("test")
            .arg(&root)
            .arg("--output")
            .arg(&out_dir)
            .output()
            .expect("run bynkc test");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(!out.status.success(), "{name}: `bynkc test` must fail");
        assert!(
            !stdout.contains("passed"),
            "{name}: no case may run:\n{stdout}"
        );
        assert!(!out_dir.exists(), "{name}: nothing may be emitted");
    }
}
