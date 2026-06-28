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
//!   visible warning. Setting `BYNK_REQUIRE_TSC=1` enforces the strict CI
//!   behaviour; otherwise an unavailable tsc prints a warning and passes.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const REQUIRE_ENV: &str = "BYNK_REQUIRE_TSC";

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

/// Build a `Command` for `program`, routing through `cmd /C` on Windows so
/// npm's `.cmd` shims (`tsc.cmd`, `npx.cmd`) resolve — Rust's CreateProcess
/// deliberately refuses to run batch scripts directly (the BatBadBut
/// hardening), so a bare `Command::new("npx")` fails there.
fn base_command(program: &str) -> Command {
    if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(program);
        c
    } else {
        Command::new(program)
    }
}

fn tool_exists(name: &str) -> bool {
    // `where` is the Windows counterpart of `which`.
    let finder = if cfg!(windows) { "where" } else { "which" };
    Command::new(finder)
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

fn fixture_target(dir: &Path) -> bynkc::BuildTarget {
    let marker = dir.join("target.txt");
    if let Ok(s) = fs::read_to_string(&marker)
        && s.trim() == "workers"
    {
        return bynkc::BuildTarget::Workers;
    }
    bynkc::BuildTarget::Bundle
}

/// v0.18: read the deploy-platform marker from a fixture root, if present.
fn fixture_platform(dir: &Path) -> bynkc::Platform {
    let marker = dir.join("platform.txt");
    if let Ok(s) = fs::read_to_string(&marker)
        && s.trim() == "node"
    {
        return bynkc::Platform::Node;
    }
    bynkc::Platform::Cloudflare
}

fn compile_fixture(
    fixture_root: &Path,
    target: bynkc::BuildTarget,
) -> Result<bynkc::ProjectOutput, Vec<bynkc::CompileError>> {
    let bynk_toml = fixture_root.join("bynk.toml");
    if bynk_toml.exists() {
        let paths = bynkc::read_project_paths(fixture_root);
        bynkc::compile_project(
            &bynkc::CompileOptions::split(fixture_root.to_path_buf(), paths).target(target),
        )
        .map_err(bynkc::ProjectFailure::flatten)
    } else {
        let src_dir = fixture_root.join("src");
        let platform = fixture_platform(fixture_root);
        bynkc::compile_project(
            &bynkc::CompileOptions::single(src_dir)
                .target(target)
                .platform(platform),
        )
        .map_err(bynkc::ProjectFailure::flatten)
    }
}

fn write_outputs(out: &bynkc::ProjectOutput, root: &Path) -> std::io::Result<()> {
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
    let mut cmd = base_command(&runner.program);
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
                let rendered = bynkc::render_project_errors(&errors);
                failures.push(format!(
                    "\n=== {} ===\nexpected compile success but got errors:\n{}",
                    dir.display(),
                    rendered,
                ));
                continue;
            }
        };
        let tmp = std::env::temp_dir().join(format!(
            "bynk-tsc-verify-{}-{}",
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

/// v0.48: the embedded runtime (`emitter/runtime.ts`) — which now
/// carries the Bearer JWT verifier — must pass `tsc --strict` standalone, not
/// only transitively inside a fixture. It is self-contained (no imports), so a
/// temp dir with just `runtime.ts` + the emitted `tsconfig.json` type-checks it
/// in isolation. (The `.ts` *bindings* import the emitted adapter surface and
/// stay covered transitively by fixtures 177/201–211; their standalone scaffold
/// is a follow-up.)
#[test]
fn embedded_runtime_passes_tsc_strict_standalone() {
    let runner = match discover_tsc() {
        Some(r) => r,
        None => {
            let require = std::env::var(REQUIRE_ENV).is_ok();
            eprintln!(
                "\n!!! TYPESCRIPT VERIFICATION SKIPPED (runtime standalone) !!!\n\
                 neither `tsc` nor `npx` is on PATH.\n"
            );
            if require {
                panic!("{REQUIRE_ENV} is set but no tsc runner was found");
            }
            return;
        }
    };
    let tmp = std::env::temp_dir().join(format!("bynk-runtime-tsc-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();
    fs::write(
        tmp.join("runtime.ts"),
        bynkc::emitter::emit_runtime_module(),
    )
    .unwrap();
    fs::write(tmp.join("tsconfig.json"), bynkc::emitter::emit_tsconfig()).unwrap();
    let (ok, output) = run_tsc_in(&runner, &tmp);
    if ok {
        let _ = fs::remove_dir_all(&tmp);
    }
    assert!(
        ok,
        "embedded runtime.ts failed tsc --strict --noEmit:\n{output}"
    );
}

/// v0.104 (real-time track slice 3b): the embedded runtime must also be
/// **type-strippable** — Node runs the emitted `.ts` directly under
/// `--experimental-strip-types` on the `--inspect` debug path (and the bundle
/// test runner), which is *stricter* than `tsc`: it rejects non-erasable TS such
/// as constructor **parameter properties**, `enum`, and `namespace`. `tsc`
/// accepts all of these, so the standalone-tsc check above does not catch them; a
/// parameter property in the runtime silently broke every `--inspect` debug
/// session (the module fails to parse before any breakpoint binds). This guards
/// that class of regression cheaply and deterministically — no debugger, no hang.
#[test]
fn embedded_runtime_strips_types_under_node() {
    // Node gained `--experimental-strip-types` in 22.6; older nodes can't run the
    // check. Skip (loudly under the CI gate) when node is absent or too old.
    let node_ok = Command::new("node")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| {
            let v = String::from_utf8_lossy(&o.stdout);
            let v = v.trim().trim_start_matches('v');
            let mut it = v.split('.');
            let major: u32 = it.next()?.parse().ok()?;
            let minor: u32 = it.next()?.parse().ok()?;
            Some(major > 22 || (major == 22 && minor >= 6))
        })
        .unwrap_or(false);
    if !node_ok {
        // Strip-types is a Node *capability* gate, not the `tsc`-presence gate — so
        // this skips silently on an older Node regardless of `BYNK_REQUIRE_TSC` (CI's
        // `Test suite` runs Node 20, which predates `--experimental-strip-types`).
        // The strip-types coverage in CI comes from the Node-22 VS Code integration
        // job that runs the emitted `.ts`; this test is the fast local backstop.
        eprintln!(
            "\n!!! NODE STRIP-TYPES CHECK SKIPPED (runtime) !!!\n\
             `node` (>= 22.6, for --experimental-strip-types) is not on PATH.\n"
        );
        return;
    }
    let tmp = std::env::temp_dir().join(format!("bynk-runtime-strip-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();
    let rt = tmp.join("runtime.ts");
    fs::write(&rt, bynkc::emitter::emit_runtime_module()).unwrap();
    let out = Command::new("node")
        .arg("--experimental-strip-types")
        .arg("--check")
        .arg(&rt)
        .output()
        .expect("run node --experimental-strip-types --check");
    let ok = out.status.success();
    if ok {
        let _ = fs::remove_dir_all(&tmp);
    }
    assert!(
        ok,
        "embedded runtime.ts is not type-strippable under node --experimental-strip-types \
         (a non-erasable construct such as a constructor parameter property, `enum`, or \
         `namespace` — these break every `--inspect` debug session):\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}
