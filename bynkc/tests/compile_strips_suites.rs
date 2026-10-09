//! #1821 (ADR 0147 D3): `bynkc compile` strips every `suite` — never
//! type-checked for the build, never emitted to the deployable — while
//! `bynkc test` compiles and runs them. `compile` used to emit `out/tests/`, and
//! a project with a `system` suite then failed `tsc` on a bundle build (its
//! test module imports the workers layout).

use std::path::{Path, PathBuf};
use std::process::Command;

fn project(tag: &str, suite: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("strip-suites-{tag}"));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src/shop")).unwrap();
    std::fs::create_dir_all(root.join("tests")).unwrap();
    std::fs::write(
        root.join("bynk.toml"),
        "[project]\nname = \"m\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/shop/maths.bynk"),
        "commons shop.maths\n\nfn double(n: Int) -> Int { n + n }\n",
    )
    .unwrap();
    std::fs::write(root.join("tests/maths.bynk"), suite).unwrap();
    root
}

fn bynkc(cwd: &Path, args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_bynkc"))
        .args(args)
        .current_dir(cwd)
        .env("NO_COLOR", "1")
        .output()
        .expect("bynkc runs");
    (
        out.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

#[test]
fn compile_emits_no_suite() {
    let root = project(
        "emits",
        "suite shop.maths {\n  case \"doubles\" { expect double(3) == 6 }\n}\n",
    );
    for target in ["bundle", "workers"] {
        let out = format!("out-{target}");
        let (ok, text) = bynkc(&root, &["compile", "--target", target, "-o", &out, "."]);
        assert!(ok, "{target}: {text}");
        assert!(
            !root.join(&out).join("tests").exists(),
            "{target}: compile emitted `tests/`"
        );
    }
}

#[test]
fn a_broken_suite_does_not_fail_compile_but_check_reports_it() {
    let root = project(
        "broken",
        "suite shop.maths {\n  case \"x\" { expect nosuch(3) == 6 }\n}\n",
    );
    let (ok, text) = bynkc(&root, &["compile", "-o", "out", "."]);
    assert!(ok, "the build never type-checks a suite: {text}");
    let (ok, text) = bynkc(&root, &["check", "--format", "short", "."]);
    assert!(!ok, "check still checks suites");
    assert!(text.contains("bynk.resolve.unknown_function"), "{text}");
}

/// ADR 0147 D5: a unit and its suite in one file. The unit is emitted; the
/// suite is not.
#[test]
fn an_atomic_file_ships_its_unit_and_drops_its_suite() {
    let root = project("atomic", "");
    std::fs::remove_file(root.join("tests/maths.bynk")).unwrap();
    std::fs::write(
        root.join("src/shop/maths.bynk"),
        "commons shop.maths {\n  fn double(n: Int) -> Int { n + n }\n}\n\n\
         suite shop.maths {\n  case \"doubles\" { expect double(3) == 6 }\n}\n",
    )
    .unwrap();
    let (ok, text) = bynkc(&root, &["compile", "-o", "out", "."]);
    assert!(ok, "{text}");
    assert!(
        root.join("out/shop/maths.ts").exists(),
        "the commons is emitted"
    );
    assert!(!root.join("out/tests").exists(), "the suite is not");
}

/// A file that does not parse fails the build even if it holds a suite: it
/// cannot be told apart from production source. Only well-formed suites are
/// stripped unchecked.
#[test]
fn a_suite_that_does_not_parse_still_fails_compile() {
    let root = project(
        "syntax",
        "suite shop.maths {\n  case \"x\" { expect double(\n}\n",
    );
    let (ok, text) = bynkc(&root, &["compile", "-o", "out", "."]);
    assert!(!ok, "a malformed file fails the build: {text}");
    assert!(text.contains("bynk.parse."), "{text}");
}
