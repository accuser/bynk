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
