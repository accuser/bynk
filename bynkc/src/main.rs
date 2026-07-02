//! bynkc — the Bynk v0.3 compiler CLI.

use std::path::{Path, PathBuf};
use std::process::{Command as ProcCommand, ExitCode, Stdio};

use bynkc::BuildTarget;
use bynkc::cli::{Cli, Command, DiagFormat, EmitFormat, TestFormat};
use bynkc::test_json::{Case, Location, Suite, TestRun};
use clap::Parser;

/// Root a directory project the way every project command should (#46): a
/// `bynk.toml` or a `src/` subdir selects **project** mode, whose flat
/// `[paths] include`/`exclude` layout (v0.113, DECISION S) defaults to the
/// conventional roots that exist (`src`, `tests`) or the project root itself;
/// otherwise the legacy **single-tree** where `<dir>` is itself the root.
/// `check`, `compile`, and `test` all route through this so the conventional
/// layout works the same from any of them. Test-ness is structural — a `suite`
/// is discovered wherever it sits and stripped from the production build — so
/// tests need no dedicated directory.
fn project_options(input: &Path) -> bynkc::CompileOptions {
    if input.join("bynk.toml").exists() || input.join("src").is_dir() {
        bynkc::CompileOptions::split(input.to_path_buf(), bynkc::read_project_paths(input))
    } else {
        bynkc::CompileOptions::single(input.to_path_buf())
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Compile {
            input,
            output,
            target,
            platform,
            emit,
        } => run_compile(input, output, target.into(), platform.into(), emit),
        Command::Check { input, format } => run_check(input, format),
        Command::Fmt { inputs, check } => run_fmt(inputs, check),
        Command::Test {
            input,
            output,
            no_run,
            format,
            inspect,
            seed,
        } => run_test(input, output, no_run, format, inspect, seed),
    }
}

/// Normalise a `--seed` value (`0x5f3a` or `5f3a`) to the bare-hex form the
/// runner reads from `BYNK_TEST_SEED` (JS `parseInt(_, 16)` does not accept a
/// `0x` prefix). Returns `None` for a non-hex value, so a typo is ignored rather
/// than silently seeding to zero.
fn normalise_seed(raw: &str) -> Option<String> {
    let hex = raw
        .strip_prefix("0x")
        .or_else(|| raw.strip_prefix("0X"))
        .unwrap_or(raw);
    if hex.is_empty() || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(hex.to_string())
}

/// In `--format json` mode the deterministic surface is the document on stdout,
/// so a `bynkc test:` line on stderr is fine but must never reach stdout.
fn run_test(
    input: PathBuf,
    output: Option<PathBuf>,
    no_run: bool,
    format: TestFormat,
    inspect: bool,
    seed: Option<String>,
) -> ExitCode {
    let json = matches!(format, TestFormat::Json);
    // v0.114: the root seed for generative `property` tests, threaded to the
    // runner via `BYNK_TEST_SEED` (bare hex). An unparseable value is dropped
    // with a warning so a run still proceeds with a fresh seed.
    let seed_hex = match seed.as_deref() {
        Some(raw) => match normalise_seed(raw) {
            Some(hex) => Some(hex),
            None => {
                if !json {
                    eprintln!("bynkc test: ignoring --seed `{raw}` (not a hex value like 0x5f3a)");
                }
                None
            }
        },
        None => None,
    };
    let output_root = output.unwrap_or_else(|| input.join("out"));
    if !input.is_dir() {
        eprintln!(
            "bynkc test: input `{}` must be a project directory containing `.bynk` files",
            input.display()
        );
        return ExitCode::FAILURE;
    }
    // v0.9.1: rooting strategy (#46: now shared with check/compile via
    // `project_options`) — a `bynk.toml` or `src/` subdir selects split-paths
    // mode (sources under `[paths] src`, tests under `[paths] tests`); else the
    // legacy single-tree where `<input>` is both the source and tests root.
    // `--inspect` compiles a debug build: `.ts` import specifiers so the emitted
    // entry runs directly under Node's strip-only type-stripping (slice 2), where
    // slice 1's source maps apply unchanged. A normal run keeps `.js` specifiers
    // for the `tsc → node` path.
    let options = {
        let o = project_options(&input);
        if inspect {
            o.import_ext(bynkc::ImportExt::Ts)
        } else {
            o
        }
    };
    let out = match bynkc::compile_project(&options) {
        Ok(out) => out,
        Err(failure) => {
            if json {
                print!(
                    "{}",
                    TestRun::compile_error(bynkc::project_failure_short_lines(&failure)).render()
                );
            } else {
                bynkc::print_project_failure(&failure);
            }
            return ExitCode::FAILURE;
        }
    };
    // v0.67: `--no-run --format json` is pure discovery — render the suite/case
    // manifest the compile retained and stop. No TS is written, no `tsc`/`node`
    // runs, and the integration workers re-compile below is skipped (the manifest
    // already carries integration suites from the compile above). A compile
    // failure took the `compile`-error path above, exactly as a run would.
    if no_run && json {
        print!("{}", TestRun::discovered(discovery_suites(&out)).render());
        return ExitCode::SUCCESS;
    }

    // Write every compiled file to disk under the output root.
    let mut wrote_any_test = false;
    let mut has_integration = false;
    for file in &out.files {
        // Map-aware write (slice 2): carries the `.ts.map` siblings + trailers so
        // a debug run (`--inspect`) can resolve `.bynk` breakpoints. Harmless for a
        // normal run, which transpiles via `tsc` and ignores the trailer.
        if let Err(e) = bynkc::write_compiled_file(file, &output_root) {
            eprintln!(
                "bynkc test: could not write `{}`: {e}",
                output_root.join(&file.output_path).display()
            );
            return ExitCode::FAILURE;
        }
        let rel = file.output_path.to_string_lossy();
        if rel.starts_with("tests/") {
            wrote_any_test = true;
        }
        if rel.starts_with("tests/integration_") {
            has_integration = true;
        }
    }

    // v0.16: integration tests stand their participants up as real Workers, so
    // they import the workers-mode output (`workers/**`) and the serialise/
    // deserialise helpers the workers commons emit. The bundle compile above
    // does not produce those, so run a second compile in workers mode and
    // overlay everything except the `tests/` tree (whose unit modules import
    // the bundle output). The workers commons are a strict superset of the
    // bundle ones, so overwriting them is safe for the bundle code too.
    if has_integration {
        let workers_out =
            bynkc::compile_project(&project_options(&input).target(bynkc::BuildTarget::Workers));
        let workers_out = match workers_out {
            Ok(o) => o,
            Err(failure) => {
                if json {
                    print!(
                        "{}",
                        TestRun::compile_error(bynkc::project_failure_short_lines(&failure))
                            .render()
                    );
                } else {
                    bynkc::print_project_failure(&failure);
                }
                return ExitCode::FAILURE;
            }
        };
        for file in &workers_out.files {
            if file.output_path.to_string_lossy().starts_with("tests/") {
                continue;
            }
            if let Err(e) = bynkc::write_compiled_file(file, &output_root) {
                eprintln!(
                    "bynkc test: could not write `{}`: {e}",
                    output_root.join(&file.output_path).display()
                );
                return ExitCode::FAILURE;
            }
        }
    }

    if !wrote_any_test {
        if json {
            print!("{}", empty_run().render());
        } else {
            eprintln!(
                "bynkc test: no test declarations found in `{}`",
                input.display()
            );
        }
        return ExitCode::SUCCESS;
    }

    let main_ts = output_root.join("tests").join("main.ts");
    if no_run {
        // Rich `--no-run` is the CI emit helper: write the runner modules and
        // report where they landed. (JSON `--no-run` already returned above with
        // the discovery document — it never reaches here.)
        eprintln!("bynkc test: tests emitted to {}", main_ts.display());
        return ExitCode::SUCCESS;
    }

    // Slice 2 (ADR 0104): launch the emitted `.ts` test entry directly under
    // Node's inspector. No `tsc` — the `.ts` runs under line-preserving
    // type-stripping, so the source maps written above resolve `.bynk`
    // breakpoints. Node prints its inspector URL; a debugger attaches there.
    if inspect {
        return run_inspect(&main_ts, seed_hex.as_deref());
    }

    let tsconfig = output_root.join("tsconfig.json");
    // Preferred: `tsc -p out/tsconfig.json` → `node out-js/tests/main.js`.
    // tsc gives us full type-checking before execution and matches what a
    // production deployment build would do. If tsc is missing, fall back to
    // tsx, which compiles-and-runs in one step. We also try npx-mediated
    // variants so a developer with `npm` available doesn't need a global
    // install. If nothing works, emit an actionable error message.
    let out_js_root = output_root
        .parent()
        .map(|p| p.join("out-js"))
        .unwrap_or_else(|| PathBuf::from("out-js"));
    let main_js = out_js_root.join("tests").join("main.js");

    // Try a sequence of (program, prefix args) tsc invocations. In JSON mode the
    // tsc step is captured so its output never reaches stdout (the document is
    // the only thing on stdout); a tsc failure on bynkc's own emitted TS is a
    // toolchain/internal problem, surfaced as a `runtime` error.
    let tsc_runners: Vec<(&str, Vec<&str>)> = vec![
        ("tsc", vec![]),
        ("npx", vec!["--yes", "-p", "typescript", "tsc"]),
    ];
    for (prog, prefix) in &tsc_runners {
        if !tool_exists(prog) {
            continue;
        }
        let mut cmd = ProcCommand::new(prog);
        for p in prefix {
            cmd.arg(p);
        }
        cmd.arg("-p").arg(&tsconfig);
        let tsc_ok = if json {
            match cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).output() {
                Ok(out) if out.status.success() => true,
                Ok(out) => {
                    print!(
                        "{}",
                        TestRun::runtime_error(
                            "tsc rejected the generated TypeScript",
                            Some(String::from_utf8_lossy(&out.stderr).into_owned()),
                        )
                        .render()
                    );
                    return ExitCode::FAILURE;
                }
                Err(_) => continue,
            }
        } else {
            match cmd
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .status()
            {
                Ok(s) if s.success() => true,
                Ok(_) => {
                    eprintln!(
                        "bynkc test: tsc reported errors against {}",
                        tsconfig.display()
                    );
                    return ExitCode::FAILURE;
                }
                Err(_) => continue,
            }
        };
        if tsc_ok {
            let mut node_cmd = ProcCommand::new("node");
            node_cmd.arg(&main_js);
            return match finish_runner(node_cmd, json, seed_hex.as_deref()) {
                Ok(code) => code,
                Err(e) => {
                    if json {
                        print!(
                            "{}",
                            TestRun::runtime_error(format!("could not run node: {e}"), None)
                                .render()
                        );
                    } else {
                        eprintln!(
                            "bynkc test: tsc succeeded but `node {}` failed: {e}",
                            main_js.display()
                        );
                    }
                    ExitCode::FAILURE
                }
            };
        }
    }

    // tsx fallback chain.
    let tsx_runners: Vec<(&str, Vec<&str>)> = vec![("tsx", vec![]), ("npx", vec!["--yes", "tsx"])];
    for (prog, prefix) in &tsx_runners {
        if !tool_exists(prog) {
            continue;
        }
        let mut cmd = ProcCommand::new(prog);
        for p in prefix {
            cmd.arg(p);
        }
        cmd.arg(&main_ts);
        match finish_runner(cmd, json, seed_hex.as_deref()) {
            Ok(code) => return code,
            Err(_) => continue,
        }
    }

    if json {
        print!(
            "{}",
            TestRun::runtime_error(
                "no test runner found: requires `tsc` (with Node.js) or `tsx` on PATH",
                None
            )
            .render()
        );
    } else {
        eprintln!(
            "bynkc test: requires either `tsc` (with Node.js) or `tsx` on PATH. \
             Install one of:\n  - `npm install -g typescript` (provides tsc; requires Node.js to run output)\n  - `npm install -g tsx` (compiles and runs TypeScript in one step)\n  Or run inside a project where `npx tsc` / `npx tsx` resolves.",
        );
    }
    ExitCode::FAILURE
}

/// A normal run with no suites — the JSON-mode document for a project with no
/// tests, or `--no-run`.
fn empty_run() -> TestRun {
    TestRun::empty()
}

/// v0.67: map the compile's retained test manifest into discovery [`Suite`]s for
/// the `--no-run --format json` document. Each case is `outcome: "discovered"`,
/// carrying its declaration `location` (when known) for editor click-through.
fn discovery_suites(out: &bynkc::ProjectOutput) -> Vec<Suite> {
    out.discovered
        .iter()
        .map(|s| Suite {
            name: s.name.clone(),
            kind: s.kind.to_string(),
            cases: s
                .cases
                .iter()
                .map(|c| Case {
                    name: c.name.clone(),
                    outcome: "discovered".to_string(),
                    message: None,
                    location: c.location.as_ref().map(|l| Location {
                        path: l.path.clone(),
                        line: l.line,
                        col: l.col,
                    }),
                })
                .collect(),
        })
        .collect()
}

/// Execute the built runner command and produce its exit code. In JSON mode the
/// runner's stdout (NDJSON) and stderr are captured, folded into the pinned
/// document, and printed; otherwise stdio is inherited so the human ✓ / ✗ output
/// flows straight through. Either way the **exit code follows the runner's own
/// process status**, so a mid-run crash (a complete NDJSON prefix but no
/// `run-end`) is never reported as success.
fn finish_runner(
    mut cmd: ProcCommand,
    json: bool,
    seed_hex: Option<&str>,
) -> std::io::Result<ExitCode> {
    if let Some(hex) = seed_hex {
        cmd.env("BYNK_TEST_SEED", hex);
    }
    if json {
        cmd.env("BYNK_TEST_FORMAT", "ndjson");
        let out = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).output()?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let doc = bynkc::test_json::parse_ndjson(&stdout).into_document(&stderr);
        print!("{}", doc.render());
        Ok(exit_from(out.status.success()))
    } else {
        let status = cmd
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()?;
        Ok(exit_from(status.success()))
    }
}

fn exit_from(success: bool) -> ExitCode {
    if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Slice 2 (ADR 0104): launch the emitted `.ts` test entry under Node's inspector
/// and hand off. Node prints its inspector `ws://` URL to stderr and pauses at the
/// first line (`--inspect-brk`) until a JavaScript debugger attaches; breakpoints
/// set in `.bynk` resolve through the emitted source maps. `--experimental-strip-types`
/// runs the `.ts` directly under line-preserving type-stripping (Node ≥ 22.6;
/// unflagged ≥ 23.6) — no `tsc`, so slice 1's `.ts.map` applies to the running file.
fn run_inspect(entry: &Path, seed_hex: Option<&str>) -> ExitCode {
    if !tool_exists("node") {
        eprintln!("bynkc test --inspect: `node` was not found on PATH");
        return ExitCode::FAILURE;
    }
    eprintln!("bynkc test --inspect: launching the test runner under Node's inspector.");
    eprintln!("  Attach a JavaScript debugger to the inspector URL below; breakpoints set");
    eprintln!("  in `.bynk` sources resolve through the emitted source maps.");
    eprintln!("  (Requires Node \u{2265} 22.6 for TypeScript type-stripping.)");
    let mut cmd = ProcCommand::new("node");
    if let Some(hex) = seed_hex {
        cmd.env("BYNK_TEST_SEED", hex);
    }
    cmd.arg("--experimental-strip-types")
        .arg("--inspect-brk")
        .arg(entry);
    match cmd
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
    {
        Ok(s) => exit_from(s.success()),
        Err(e) => {
            eprintln!("bynkc test --inspect: could not run node: {e}");
            ExitCode::FAILURE
        }
    }
}

fn tool_exists(name: &str) -> bool {
    // `which` is POSIX; on Windows we'd use `where`, but the rest of the
    // toolchain has been Unix-only.
    ProcCommand::new("which")
        .arg(name)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn run_fmt(inputs: Vec<PathBuf>, check: bool) -> ExitCode {
    use bynkc::fmt::{FormatOptions, format_source};
    let opts = FormatOptions::default();
    if inputs.is_empty() {
        eprintln!("bynkc fmt: no input files (pass file paths or `-` for stdin)");
        return ExitCode::FAILURE;
    }
    let mut had_diff = false;
    let mut had_error = false;
    for input in &inputs {
        if input.as_os_str() == "-" {
            use std::io::Read;
            let mut source = String::new();
            if let Err(e) = std::io::stdin().read_to_string(&mut source) {
                eprintln!("bynkc fmt: read from stdin: {e}");
                return ExitCode::FAILURE;
            }
            match format_source(&source, &opts) {
                Ok(formatted) => print!("{formatted}"),
                Err(e) => {
                    bynkc::print_errors(&e.errors, &source, "<stdin>");
                    return ExitCode::FAILURE;
                }
            }
            continue;
        }
        let source = match std::fs::read_to_string(input) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("bynkc fmt: read `{}`: {e}", input.display());
                had_error = true;
                continue;
            }
        };
        let filename = input.display().to_string();
        match format_source(&source, &opts) {
            Ok(formatted) => {
                if check {
                    if formatted != source {
                        eprintln!(
                            "bynkc fmt: {} is not canonically formatted",
                            input.display()
                        );
                        had_diff = true;
                    }
                } else if formatted != source
                    && let Err(e) = std::fs::write(input, formatted)
                {
                    eprintln!("bynkc fmt: write `{}`: {e}", input.display());
                    had_error = true;
                }
            }
            Err(e) => {
                bynkc::print_errors(&e.errors, &source, &filename);
                had_error = true;
            }
        }
    }
    if had_error || (check && had_diff) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn run_compile(
    input: PathBuf,
    output: PathBuf,
    target: BuildTarget,
    platform: bynkc::Platform,
    emit: EmitFormat,
) -> ExitCode {
    if input.is_dir() {
        // Multi-file project compile.
        match bynkc::compile_project(&project_options(&input).target(target).platform(platform)) {
            Ok(out) => {
                // `--emit js` (the in-browser track, slice 1, ADR 0137): the
                // emitter always produces TypeScript; a JS artefact is that same
                // output with types stripped (the emitter is strip-only — ADR
                // 0136 — so the strip is total). Rewrite the file set before the
                // shared writer touches disk.
                let out = match emit {
                    EmitFormat::Ts => out,
                    EmitFormat::Js => match bynkc::strip_project_to_js(out) {
                        Ok(out) => out,
                        Err(e) => {
                            eprintln!("bynkc: could not produce JavaScript output: {e}");
                            return ExitCode::FAILURE;
                        }
                    },
                };
                match bynkc::write_output(&out, &output) {
                    Ok(()) => {
                        // ADR 0117: surface non-failing warnings; the build succeeds.
                        bynkc::print_project_warnings(&out.warnings);
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        eprintln!(
                            "bynkc: could not write output under `{}`: {e}",
                            output.display()
                        );
                        ExitCode::FAILURE
                    }
                }
            }
            Err(failure) => {
                bynkc::print_project_failure(&failure);
                ExitCode::FAILURE
            }
        }
    } else {
        let source = match std::fs::read_to_string(&input) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("bynkc: could not read `{}`: {e}", input.display());
                return ExitCode::FAILURE;
            }
        };
        let filename = input.display().to_string();
        match bynkc::compile_with_warnings(&source, &filename) {
            Ok(compiled) => {
                // For `--emit js` the single emitted module is stripped to JS; the
                // caller names the output path (e.g. `-o out.js`).
                let body = match emit {
                    EmitFormat::Ts => compiled.ts,
                    EmitFormat::Js => match bynkc::strip_types(&compiled.ts, &filename) {
                        Ok(js) => js,
                        Err(e) => {
                            eprintln!("bynkc: could not produce JavaScript output: {e}");
                            return ExitCode::FAILURE;
                        }
                    },
                };
                if let Some(parent) = output.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Err(e) = std::fs::write(&output, body) {
                    eprintln!("bynkc: could not write `{}`: {e}", output.display());
                    return ExitCode::FAILURE;
                }
                // ADR 0117: render non-failing warnings with source context.
                if !compiled.warnings.is_empty() {
                    bynkc::print_errors(&compiled.warnings, &source, &filename);
                }
                ExitCode::SUCCESS
            }
            Err(errors) => {
                bynkc::print_errors(&errors, &source, &filename);
                ExitCode::FAILURE
            }
        }
    }
}

fn run_check(input: PathBuf, format: DiagFormat) -> ExitCode {
    let short = format == DiagFormat::Short;
    if input.is_dir() {
        match bynkc::compile_project(&project_options(&input)) {
            Ok(out) => {
                bynkc::print_project_warnings(&out.warnings);
                ExitCode::SUCCESS
            }
            Err(failure) => {
                if short {
                    bynkc::print_project_failure_short(&failure);
                } else {
                    bynkc::print_project_failure(&failure);
                }
                ExitCode::FAILURE
            }
        }
    } else {
        let source = match std::fs::read_to_string(&input) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("bynkc: could not read `{}`: {e}", input.display());
                return ExitCode::FAILURE;
            }
        };
        let filename = input.display().to_string();
        match bynkc::compile_with_warnings(&source, &filename) {
            Ok(compiled) => {
                if !compiled.warnings.is_empty() {
                    if short {
                        bynkc::print_errors_short(&compiled.warnings, &source, &filename);
                    } else {
                        bynkc::print_errors(&compiled.warnings, &source, &filename);
                    }
                }
                ExitCode::SUCCESS
            }
            Err(errors) => {
                if short {
                    bynkc::print_errors_short(&errors, &source, &filename);
                } else {
                    bynkc::print_errors(&errors, &source, &filename);
                }
                ExitCode::FAILURE
            }
        }
    }
}
