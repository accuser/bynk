//! v0.9.1: TypeScript verification of emitted output.
//!
//! For every project-form positive fixture, compile to a temp directory and
//! run `tsc --strict --noEmit` over the result. Any TypeScript error fails
//! the fixture and surfaces the raw `tsc` output.
//!
//! This catches emitter bugs that visual review (and the snapshot tests)
//! miss — it's the backstop for every emission change. Per the v0.9.1 spec
//! §4.3:
//!
//! - If `tsc` is unavailable in the environment, the stage **must skip
//!   loudly** so a CI breakage isn't quietly mistaken for green.
//! - In CI the skip is a hard failure; locally it's permitted with a
//!   visible warning. Setting `KARN_REQUIRE_TSC=1` enforces the strict CI
//!   behaviour; otherwise an unavailable tsc prints a warning and passes.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const REQUIRE_ENV: &str = "KARN_REQUIRE_TSC";

#[derive(Clone)]
struct TscRunner {
    program: String,
    args: Vec<String>,
}

fn discover_tsc() -> Option<TscRunner> {
    if tool_exists("tsc") {
        return Some(TscRunner {
            program: "tsc".to_string(),
            args: vec![],
        });
    }
    if tool_exists("npx") {
        // Pin TypeScript to avoid surprising upgrades. The compiler emits
        // ES2022 + NodeNext output; tsc 5.x supports this.
        return Some(TscRunner {
            program: "npx".to_string(),
            args: vec![
                "--yes".to_string(),
                "-p".to_string(),
                "typescript@5".to_string(),
                "tsc".to_string(),
            ],
        });
    }
    None
}

fn tool_exists(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn fixture_dirs(category: &str) -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(category);
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(&root) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

fn fixture_target(dir: &Path) -> karnc::BuildTarget {
    let marker = dir.join("target.txt");
    if let Ok(s) = fs::read_to_string(&marker)
        && s.trim() == "workers"
    {
        return karnc::BuildTarget::Workers;
    }
    karnc::BuildTarget::Bundle
}

fn compile_fixture(
    fixture_root: &Path,
    target: karnc::BuildTarget,
) -> Result<karnc::ProjectOutput, Vec<karnc::CompileError>> {
    let karn_toml = fixture_root.join("karn.toml");
    if karn_toml.exists() {
        let paths = karnc::read_project_paths(fixture_root);
        karnc::compile_project_with_split_paths(fixture_root, target, &paths)
    } else {
        let src_dir = fixture_root.join("src");
        karnc::compile_project_with_target(&src_dir, target)
    }
}

fn write_outputs(out: &karnc::ProjectOutput, root: &Path) -> std::io::Result<()> {
    for file in &out.files {
        let target = root.join(&file.output_path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        // Only write TypeScript / JSON / TOML artefacts; `tsc` will
        // process the .ts and read tsconfig.json. wrangler.toml is left
        // in place but ignored by tsc.
        fs::write(&target, &file.typescript)?;
    }
    Ok(())
}

fn run_tsc_in(runner: &TscRunner, project_dir: &Path) -> (bool, String) {
    let mut cmd = Command::new(&runner.program);
    for a in &runner.args {
        cmd.arg(a);
    }
    cmd.arg("--strict")
        .arg("--noEmit")
        .arg("-p")
        .arg(project_dir.join("tsconfig.json"));
    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => return (false, format!("could not launch tsc: {e}")),
    };
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let combined = if stderr.is_empty() {
        stdout
    } else if stdout.is_empty() {
        stderr
    } else {
        format!("{stdout}\n{stderr}")
    };
    (output.status.success(), combined)
}

/// Run `tsc --strict --noEmit` against every project-form positive fixture's
/// emitted TypeScript. Fixtures that compile but don't type-check are
/// failures here.
#[test]
fn emitted_typescript_passes_tsc_strict() {
    let runner = match discover_tsc() {
        Some(r) => r,
        None => {
            let require = std::env::var(REQUIRE_ENV).is_ok();
            eprintln!(
                "\n!!! TYPESCRIPT VERIFICATION SKIPPED !!!\n\
                 neither `tsc` nor `npx` is on PATH.\n\
                 Emitted TypeScript was NOT type-checked.\n\
                 Install TypeScript locally (`npm i -g typescript`) or run in an environment with `npx`.\n"
            );
            if require {
                panic!(
                    "{REQUIRE_ENV} is set but no tsc runner was found — refusing to skip TypeScript verification",
                );
            }
            return;
        }
    };

    let dirs = fixture_dirs("positive");
    assert!(!dirs.is_empty(), "no positive fixtures found");

    let mut failures: Vec<String> = Vec::new();
    let mut checked = 0usize;
    for dir in &dirs {
        let src_dir = dir.join("src");
        // Only project-form fixtures emit a tsconfig.json + runtime.ts; the
        // single-file fixtures emit one bare .ts and aren't a complete
        // module graph.
        if !src_dir.is_dir() {
            continue;
        }
        let target = fixture_target(dir);
        let compiled = match compile_fixture(dir, target) {
            Ok(out) => out,
            Err(errors) => {
                let rendered = karnc::render_project_errors(&errors);
                failures.push(format!(
                    "\n=== {} ===\nexpected compile success but got errors:\n{}",
                    dir.display(),
                    rendered,
                ));
                continue;
            }
        };
        let tmp = std::env::temp_dir().join(format!(
            "karn-tsc-verify-{}-{}",
            dir.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("fixture"),
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&tmp);
        if let Err(e) = write_outputs(&compiled, &tmp) {
            failures.push(format!(
                "\n=== {} ===\nfailed to write emitted output to {}: {e}",
                dir.display(),
                tmp.display(),
            ));
            continue;
        }
        let (ok, output) = run_tsc_in(&runner, &tmp);
        if !ok {
            failures.push(format!(
                "\n=== {} ===\n--- tsc --strict --noEmit reported errors ---\n{}",
                dir.display(),
                output,
            ));
        } else {
            checked += 1;
        }
        // Best-effort cleanup; leave on failure for inspection.
        if ok {
            let _ = fs::remove_dir_all(&tmp);
        }
    }
    assert!(
        checked > 0,
        "no project-form positive fixtures were tsc-checked"
    );
    if !failures.is_empty() {
        panic!(
            "tsc verification failed ({} fixtures clean, {} failed):\n{}",
            checked,
            failures.len(),
            failures.join("\n"),
        );
    }
}
