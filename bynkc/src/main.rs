//! bynkc — the Bynk v0.3 compiler CLI.

use std::path::{Path, PathBuf};
use std::process::{Command as ProcCommand, ExitCode, Stdio};

use bynkc::BuildTarget;
use bynkc::cli::{Cli, Command, DiagFormat, TestFormat};
use bynkc::test_json::TestRun;
use clap::Parser;

/// Root a directory project the way every project command should (#46): a
/// `bynk.toml` or a `src/` subdir selects **split-paths** mode (sources and
/// tests under `[paths]`, defaults `src`/`tests`); otherwise the legacy
/// **single-tree** where `<dir>` is itself the source root. `check`, `compile`,
/// and `test` all route through this so the conventional layout works the same
/// from any of them (`bynkc check .` no longer differs from `bynkc test .`).
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
        } => run_compile(input, output, target.into(), platform.into()),
        Command::Check { input, format } => run_check(input, format),
        Command::Fmt { inputs, check } => run_fmt(inputs, check),
        Command::Test {
            input,
            output,
            no_run,
            format,
        } => run_test(input, output, no_run, format),
    }
}

/// In `--format json` mode the deterministic surface is the document on stdout,
/// so a `bynkc test:` line on stderr is fine but must never reach stdout.
fn run_test(input: PathBuf, output: Option<PathBuf>, no_run: bool, format: TestFormat) -> ExitCode {
    let json = matches!(format, TestFormat::Json);
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
    let out = match bynkc::compile_project(&project_options(&input)) {
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
    // Write every compiled file to disk under the output root.
    let mut wrote_any_test = false;
    let mut has_integration = false;
    for file in &out.files {
        let target = output_root.join(&file.output_path);
        if let Some(parent) = target.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::fs::write(&target, &file.typescript) {
            eprintln!("bynkc test: could not write `{}`: {e}", target.display());
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
            let target = output_root.join(&file.output_path);
            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(e) = std::fs::write(&target, &file.typescript) {
                eprintln!("bynkc test: could not write `{}`: {e}", target.display());
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
        // Discovery without running is deferred (proposal v0.59); a JSON
        // consumer still gets a valid (empty) document.
        if json {
            print!("{}", empty_run().render());
        } else {
            eprintln!("bynkc test: tests emitted to {}", main_ts.display());
        }
        return ExitCode::SUCCESS;
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
            return match finish_runner(node_cmd, json) {
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
        match finish_runner(cmd, json) {
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

/// Execute the built runner command and produce its exit code. In JSON mode the
/// runner's stdout (NDJSON) and stderr are captured, folded into the pinned
/// document, and printed; otherwise stdio is inherited so the human ✓ / ✗ output
/// flows straight through. Either way the **exit code follows the runner's own
/// process status**, so a mid-run crash (a complete NDJSON prefix but no
/// `run-end`) is never reported as success.
fn finish_runner(mut cmd: ProcCommand, json: bool) -> std::io::Result<ExitCode> {
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
) -> ExitCode {
    if input.is_dir() {
        // Multi-file project compile.
        match bynkc::compile_project(&project_options(&input).target(target).platform(platform)) {
            Ok(out) => {
                for file in &out.files {
                    let target = output.join(&file.output_path);
                    if let Some(parent) = target.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    if let Err(e) = std::fs::write(&target, &file.typescript) {
                        eprintln!("bynkc: could not write `{}`: {e}", target.display());
                        return ExitCode::FAILURE;
                    }
                }
                ExitCode::SUCCESS
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
        match bynkc::compile(&source, &filename) {
            Ok(ts) => {
                if let Some(parent) = output.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Err(e) = std::fs::write(&output, ts) {
                    eprintln!("bynkc: could not write `{}`: {e}", output.display());
                    return ExitCode::FAILURE;
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
            Ok(_) => ExitCode::SUCCESS,
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
        match bynkc::compile(&source, &filename) {
            Ok(_) => ExitCode::SUCCESS,
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
