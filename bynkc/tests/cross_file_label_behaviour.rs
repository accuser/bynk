//! A diagnostic's secondary label that lies in a sibling file of a multi-file
//! unit renders in a snippet of *that* file. Before, the renderer drew every
//! label against the diagnostic's own file, so a duplicate service's
//! "previously declared here" — declared in `a_web.bynk` — was underlined in
//! `z_more.bynk`'s snippet, at the same byte offsets.
//!
//! Drives the real `bynkc` CLI on a throwaway project: `check` renders through
//! the project-check printer and `compile` through the project-failure printer.

use std::path::PathBuf;
use std::process::Command;

/// A throwaway on-disk project, removed on drop (including on panic).
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const SERVICE: &str = "service api {\n  on call() -> Effect[Int] {\n    Effect.pure(1)\n  }\n}\n";

fn duplicate_service_project(tag: &str) -> Scratch {
    let dir = std::env::temp_dir().join(format!(
        "bynk_cross_file_label_{tag}_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src/app")).unwrap();
    std::fs::write(
        dir.join("bynk.toml"),
        "[project]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    // The service sits on line 3 of `a_web.bynk` and line 6 of `z_more.bynk`.
    std::fs::write(
        dir.join("src/app/a_web.bynk"),
        format!("context app\n\n{SERVICE}"),
    )
    .unwrap();
    std::fs::write(
        dir.join("src/app/z_more.bynk"),
        format!("context app\n\n\n\n\n{SERVICE}"),
    )
    .unwrap();
    Scratch(dir)
}

fn run(subcommand: &str, project: &Scratch) -> String {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_bynkc"));
    cmd.arg(subcommand).arg(&project.0);
    if subcommand == "compile" {
        cmd.arg("--output").arg(project.0.join("out"));
    }
    let out = cmd
        .current_dir(&project.0)
        .env("NO_COLOR", "1")
        .output()
        .expect("run bynkc");
    assert!(
        !out.status.success(),
        "a duplicate service must fail `{subcommand}`"
    );
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The report's snippets, each as its `file:line:col` header and body.
fn snippets(report: &str) -> Vec<(&str, &str)> {
    let mut parts: Vec<(&str, &str)> = Vec::new();
    let mut rest = report;
    while let Some(open) = rest.find("─[ ") {
        let after = &rest[open + "─[ ".len()..];
        let close = after.find(" ]").expect("a snippet header closes");
        let header = &after[..close];
        let body = &after[close..];
        let end = body.find("─[ ").unwrap_or(body.len());
        parts.push((header, &body[..end]));
        rest = &body[end..];
    }
    parts
}

fn assert_label_in_its_own_file(subcommand: &str) {
    let project = duplicate_service_project(subcommand);
    let stderr = run(subcommand, &project);
    let report = stderr
        .split("[bynk.resolve.duplicate_service]")
        .nth(1)
        .unwrap_or_else(|| panic!("expected a duplicate_service report, got:\n{stderr}"));
    let report = report.split("\n[bynk.").next().unwrap();
    let parts = snippets(report);
    assert_eq!(parts.len(), 2, "one snippet per file, got:\n{report}");
    let (primary, primary_body) = parts[0];
    let (sibling, sibling_body) = parts[1];
    assert!(
        primary.ends_with("src/app/z_more.bynk:6:9"),
        "got:\n{report}"
    );
    assert!(
        !primary_body.contains("previously declared here"),
        "the label must not be drawn in z_more.bynk's snippet, got:\n{report}"
    );
    assert!(
        sibling.ends_with("src/app/a_web.bynk:3:9"),
        "got:\n{report}"
    );
    assert!(
        sibling_body.contains(" 3 │ service api {")
            && sibling_body.contains("previously declared here"),
        "the label must be drawn on a_web.bynk's line 3, got:\n{report}"
    );
}

#[test]
fn check_renders_a_sibling_files_label_in_that_file() {
    assert_label_in_its_own_file("check");
}

#[test]
fn compile_renders_a_sibling_files_label_in_that_file() {
    assert_label_in_its_own_file("compile");
}
