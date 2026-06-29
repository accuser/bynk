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

/// Whether the local `node` exists and is recent enough to be worth invoking for
/// a strip check. The committed `strip_check.mjs` harness performs the precise
/// API gate (`stripTypeScriptTypes`, Node ≥ 22.13) and exits 2 when it is
/// unavailable; this coarse check just avoids spawning an ancient/absent Node.
fn node_present() -> bool {
    Command::new("node")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

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
    run_tsc_with_config(runner, &project_dir.join("tsconfig.json"))
}

fn run_tsc_with_config(runner: &TscRunner, tsconfig: &Path) -> (bool, String) {
    let mut cmd = base_command(&runner.program);
    for a in &runner.args {
        cmd.arg(a);
    }
    cmd.arg("--strict").arg("--noEmit").arg("-p").arg(tsconfig);
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
///
/// **One `tsc` for all fixtures.** Spawning `tsc` per fixture paid its multi-
/// second startup (process launch + `lib.*.d.ts` load + program construction)
/// ~165 times over — the dominant cost, while the actual type-check of each
/// small fixture is cheap. Instead we stage every fixture into its own subdir
/// under one temp root and type-check the whole tree in a single `tsc`
/// invocation, so that startup is paid once. The fixtures stay isolated: every
/// import in the emitted output is relative (`./`, `../`) so nothing resolves
/// across subdir boundaries, and the emitted modules declare no `declare
/// global` / ambient / triple-slash surface that would collide in one program.
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

    // One temp root; each fixture is staged into `root/<fixture-name>/…`,
    // preserving its emitted layout (its own `runtime.ts`, `src/…`, etc.).
    let root = std::env::temp_dir().join(format!("bynk-tsc-verify-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create tsc-verify temp root");

    let mut failures: Vec<String> = Vec::new();
    // Fixture names successfully staged into the tree — the set the single
    // `tsc` run actually type-checks.
    let mut staged: Vec<String> = Vec::new();
    for dir in &dirs {
        let src_dir = dir.join("src");
        // Only project-form fixtures emit a tsconfig.json + runtime.ts; the
        // single-file fixtures emit one bare .ts and aren't a complete
        // module graph.
        if !src_dir.is_dir() {
            continue;
        }
        let name = dir
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("fixture")
            .to_string();
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
        let fixture_root = root.join(&name);
        if let Err(e) = write_outputs(&compiled, &fixture_root) {
            failures.push(format!(
                "\n=== {} ===\nfailed to write emitted output to {}: {e}",
                dir.display(),
                fixture_root.display(),
            ));
            continue;
        }
        staged.push(name);
    }

    // A root `tsconfig.json` whose `include: ["**/*.ts"]` (from
    // `emit_tsconfig`) sweeps every staged fixture subdir, so one `tsc`
    // checks them all. The per-fixture `tsconfig.json` files in each subdir
    // are inert here — `tsc -p` consults only the root config.
    fs::write(root.join("tsconfig.json"), bynkc::emitter::emit_tsconfig())
        .expect("write root tsconfig");

    let (ok, output) = run_tsc_in(&runner, &root);
    if !ok {
        // Attribute each error to the fixture whose subdir name appears in the
        // diagnostic path, so a failure still points at the offending fixture
        // rather than a wall of paths.
        let mut blamed: Vec<&String> = staged
            .iter()
            .filter(|name| {
                output.contains(&format!("{name}/")) || output.contains(&format!("{name}\\"))
            })
            .collect();
        blamed.sort();
        let blame_line = if blamed.is_empty() {
            "(could not attribute errors to a specific fixture — see output)".to_string()
        } else {
            format!(
                "fixtures with errors: {}",
                blamed
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        failures.push(format!(
            "\n=== single-pass tsc --strict --noEmit reported errors ===\n{blame_line}\n{output}",
        ));
    } else {
        // Best-effort cleanup; leave on failure for inspection.
        let _ = fs::remove_dir_all(&root);
    }

    let checked = if ok { staged.len() } else { 0 };
    assert!(
        !staged.is_empty(),
        "no project-form positive fixtures were staged for tsc"
    );
    if !failures.is_empty() {
        panic!(
            "tsc verification failed ({} fixtures staged, {} clean, {} failure section(s)):\n{}",
            staged.len(),
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

/// In-browser track, slice 0 — the **strip-only emission invariant** (ADR 0136):
/// *every* `.ts` bynkc emits must be erasable by pure type-stripping, not just the
/// embedded `runtime.ts` covered above. Node runs emitted `.ts` directly under
/// `--experimental-strip-types` on the `--inspect` debug path (and the in-browser
/// eval this track is built toward), which rejects non-erasable TS — parameter
/// properties, `enum`, `namespace`. `tsc` accepts all of those, so
/// `emitted_typescript_passes_tsc_strict` cannot catch them; this test is the
/// guard that makes the invariant load-bearing across the whole emitted surface:
/// the de-sugared provider `given` constructor (`emit.rs`), the first-party
/// bindings (`bynk-{cloudflare,node}.ts`, `cloudflare.binding.ts`), the emitted
/// test scaffolding, and any user binding copied into a fixture.
///
/// Every project-form positive fixture is compiled and its emitted output staged
/// into one temp root; then each `.ts` is checked with `node
/// --experimental-strip-types --check`. Node-spawn dominates, so the per-file
/// checks fan out across worker threads.
#[test]
fn all_emitted_typescript_strips_under_node() {
    if !node_present() {
        eprintln!(
            "\n!!! NODE STRIP-TYPES CHECK SKIPPED (all emitted output) !!!\n\
             `node` is not on PATH.\n"
        );
        return;
    }

    let dirs = fixture_dirs("positive");
    assert!(!dirs.is_empty(), "no positive fixtures found");

    let root = std::env::temp_dir().join(format!("bynk-strip-verify-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create strip-verify temp root");

    let mut staged: Vec<String> = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    for dir in &dirs {
        let src_dir = dir.join("src");
        // Only project-form fixtures emit a complete module graph (their own
        // runtime.ts, bindings, test scaffolding) — the surface the invariant
        // governs. Single-file fixtures emit one bare .ts and are covered by the
        // emitter unit tests.
        if !src_dir.is_dir() {
            continue;
        }
        let name = dir
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("fixture")
            .to_string();
        let target = fixture_target(dir);
        match compile_fixture(dir, target) {
            Ok(out) => {
                let fixture_root = root.join(&name);
                if let Err(e) = write_outputs(&out, &fixture_root) {
                    failures.push(format!(
                        "\n=== {} ===\nfailed to write emitted output: {e}",
                        dir.display()
                    ));
                } else {
                    staged.push(name);
                }
            }
            Err(errors) => failures.push(format!(
                "\n=== {} ===\nexpected compile success but got errors:\n{}",
                dir.display(),
                bynkc::render_project_errors(&errors),
            )),
        }
    }
    assert!(
        failures.is_empty(),
        "staging for strip verification failed:\n{}",
        failures.join("\n")
    );
    assert!(
        !staged.is_empty(),
        "no project-form positive fixtures were staged for strip verification"
    );

    // One Node process strips the whole staged tree (see strip_check.mjs).
    let harness = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("support")
        .join("strip_check.mjs");
    let out = Command::new("node")
        .arg("--no-warnings")
        .arg(&harness)
        .arg(&root)
        .output()
        .expect("run strip_check.mjs");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    match out.status.code() {
        Some(0) => {
            let _ = fs::remove_dir_all(&root);
        }
        // `stripTypeScriptTypes` unavailable (Node < 22.13): skip loudly, like the
        // `tsc`-presence and runtime strip gates above.
        Some(2) => {
            eprintln!(
                "\n!!! NODE STRIP-TYPES CHECK SKIPPED (all emitted output) !!!\n\
                 {}\n",
                stderr.trim()
            );
            let _ = fs::remove_dir_all(&root);
        }
        _ => panic!(
            "emitted TypeScript is not strip-removable — a non-erasable construct (a constructor \
             parameter property, `enum`, or `namespace`) breaks both `--inspect` debug sessions \
             and the in-browser eval path. De-sugar it in the emitter (e.g. a declared field + \
             assigning constructor instead of a parameter property).\nFAIL <file> <code> \
             <reason>:\n{}\n{}",
            stdout.trim(),
            stderr.trim(),
        ),
    }
}

// ---------------------------------------------------------------------------
// In-browser track, slice 1 — the first-class JS artefact (ADR 0137).
//
// `bynkc compile --emit js` type-strips the emitted TypeScript to JavaScript via
// `bynkc::strip_project_to_js` (oxc). These two tests guard that path: the first
// is pure-Rust and always runs (the strip must succeed on every fixture and yield
// a JS-shaped tree); the second runs the emitted `.js` through `node --check` to
// confirm it is valid, runnable ES module syntax with no TypeScript residue.
// ---------------------------------------------------------------------------

/// Every project fixture must strip to a JavaScript artefact without error, and
/// the result must be JS-shaped: no `.ts` files survive, the `tsconfig.json` is
/// dropped, and at least one `.js` module is produced. Pure Rust — the primary
/// guard, run even where Node is absent. A strip failure here means the emitter
/// produced TypeScript oxc could not erase (it should be strip-only — ADR 0136).
#[test]
fn js_artefact_strips_every_fixture_to_js_shape() {
    let dirs = fixture_dirs("positive");
    assert!(!dirs.is_empty(), "no positive fixtures found");

    let mut checked = 0usize;
    let mut failures: Vec<String> = Vec::new();
    for dir in &dirs {
        if !dir.join("src").is_dir() {
            continue;
        }
        let name = dir
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("fixture")
            .to_string();
        let target = fixture_target(dir);
        // Compile failures are the TS tests' concern, not this one.
        let Ok(out) = compile_fixture(dir, target) else {
            continue;
        };
        match bynkc::strip_project_to_js(out) {
            Ok(js_out) => {
                checked += 1;
                let mut js_count = 0;
                for f in &js_out.files {
                    let p = f.output_path.to_string_lossy();
                    if p.ends_with(".ts") {
                        failures.push(format!("{name}: a .ts file survived JS emission: {p}"));
                    }
                    if f.output_path.file_name().and_then(|n| n.to_str()) == Some("tsconfig.json") {
                        failures.push(format!("{name}: tsconfig.json was not dropped"));
                    }
                    if p.ends_with(".js") {
                        js_count += 1;
                    }
                }
                if js_count == 0 {
                    failures.push(format!("{name}: JS emission produced no .js modules"));
                }
            }
            Err(e) => failures.push(format!("{name}: strip failed: {e}")),
        }
    }

    assert!(checked > 0, "no project-form fixtures were stripped");
    assert!(
        failures.is_empty(),
        "JS artefact issues across {checked} fixture(s):\n{}",
        failures.join("\n"),
    );
}

/// Stage every project fixture's `--emit js` output into one tree and run
/// `node --check` over every `.js` — proof the stripped output is valid, runnable
/// ES-module JavaScript (a surviving type annotation would fail the check). Node
/// is the only requirement; skips loudly when it is absent, like the `tsc` gate.
#[test]
fn emitted_javascript_is_valid_under_node() {
    if !node_present() {
        eprintln!("\n!!! NODE JS-CHECK SKIPPED (emitted JavaScript) !!!\n`node` is not on PATH.\n");
        return;
    }

    let dirs = fixture_dirs("positive");
    assert!(!dirs.is_empty(), "no positive fixtures found");

    let root = std::env::temp_dir().join(format!("bynk-js-verify-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create js-verify temp root");
    // Make Node parse the emitted `.js` as ES modules regardless of Node version.
    fs::write(root.join("package.json"), "{\n  \"type\": \"module\"\n}\n")
        .expect("write root package.json");

    let mut staged = 0usize;
    let mut failures: Vec<String> = Vec::new();
    for dir in &dirs {
        if !dir.join("src").is_dir() {
            continue;
        }
        let name = dir
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("fixture")
            .to_string();
        let target = fixture_target(dir);
        let Ok(out) = compile_fixture(dir, target) else {
            continue;
        };
        let js_out = match bynkc::strip_project_to_js(out) {
            Ok(o) => o,
            Err(e) => {
                failures.push(format!("{name}: strip failed: {e}"));
                continue;
            }
        };
        if let Err(e) = write_outputs(&js_out, &root.join(&name)) {
            failures.push(format!("{name}: failed to write JS output: {e}"));
            continue;
        }
        staged += 1;
    }
    assert!(
        failures.is_empty(),
        "staging JS output failed:\n{}",
        failures.join("\n")
    );
    assert!(
        staged > 0,
        "no project-form fixtures were staged for JS verification"
    );

    let harness = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("support")
        .join("js_check.mjs");
    let out = Command::new("node")
        .arg(&harness)
        .arg(&root)
        .output()
        .expect("run js_check.mjs");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    if out.status.success() {
        let _ = fs::remove_dir_all(&root);
    } else {
        panic!(
            "emitted JavaScript is not valid under `node --check` — a TypeScript construct survived \
             stripping, or the JS is malformed.\nFAIL <file> <reason>:\n{}\n{}",
            stdout.trim(),
            stderr.trim(),
        );
    }
}
