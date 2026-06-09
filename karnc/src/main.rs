//! karnc — the Karn v0.3 compiler CLI.

use std::path::PathBuf;
use std::process::{Command as ProcCommand, ExitCode, Stdio};

use clap::Parser;
use karnc::BuildTarget;
use karnc::cli::{Cli, Command};

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Compile {
            input,
            output,
            target,
            platform,
        } => run_compile(input, output, target.into(), platform.into()),
        Command::Check { input } => run_check(input),
        Command::Fmt { inputs, check } => run_fmt(inputs, check),
        Command::Test {
            input,
            output,
            no_run,
        } => run_test(input, output, no_run),
    }
}

fn run_test(input: PathBuf, output: Option<PathBuf>, no_run: bool) -> ExitCode {
    let output_root = output.unwrap_or_else(|| input.join("out"));
    if !input.is_dir() {
        eprintln!(
            "karnc test: input `{}` must be a project directory containing `.karn` files",
            input.display()
        );
        return ExitCode::FAILURE;
    }
    // v0.9.1: pick rooting strategy. Two cases:
    // (a) karn.toml present, or src/ subdir exists → split-paths mode rooted
    //     at <input>, with sources under [paths] src and tests under
    //     [paths] tests (defaults "src" and "tests"). Test paths are checked
    //     against their target's qualified name.
    // (b) neither → fall back to legacy single-tree mode where <input> is
    //     both the source root and the tests root.
    let karn_toml = input.join("karn.toml");
    let src_dir = input.join("src");
    let split_mode = karn_toml.exists() || src_dir.is_dir();
    let out = if split_mode {
        let paths = karnc::read_project_paths(&input);
        match karnc::compile_project_with_split_paths(&input, karnc::BuildTarget::Bundle, &paths) {
            Ok(out) => out,
            Err(errors) => {
                karnc::print_project_errors(&input, &errors);
                return ExitCode::FAILURE;
            }
        }
    } else {
        match karnc::compile_project(&input) {
            Ok(out) => out,
            Err(errors) => {
                karnc::print_project_errors(&input, &errors);
                return ExitCode::FAILURE;
            }
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
            eprintln!("karnc test: could not write `{}`: {e}", target.display());
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
        let workers_out = if split_mode {
            let paths = karnc::read_project_paths(&input);
            karnc::compile_project_with_split_paths(&input, karnc::BuildTarget::Workers, &paths)
        } else {
            karnc::compile_project_with_target(&input, karnc::BuildTarget::Workers)
        };
        let workers_out = match workers_out {
            Ok(o) => o,
            Err(errors) => {
                karnc::print_project_errors(&input, &errors);
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
                eprintln!("karnc test: could not write `{}`: {e}", target.display());
                return ExitCode::FAILURE;
            }
        }
    }

    if !wrote_any_test {
        eprintln!(
            "karnc test: no test declarations found in `{}`",
            input.display()
        );
        return ExitCode::SUCCESS;
    }

    let main_ts = output_root.join("tests").join("main.ts");
    if no_run {
        eprintln!("karnc test: tests emitted to {}", main_ts.display());
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

    // Try a sequence of (program, prefix args) tsc invocations.
    let tsc_runners: Vec<(&str, Vec<&str>)> = vec![
        ("tsc", vec![]),
        ("npx", vec!["--yes", "-p", "typescript", "tsc"]),
    ];
    let mut tsc_attempted = false;
    for (prog, prefix) in &tsc_runners {
        if !tool_exists(prog) {
            continue;
        }
        tsc_attempted = true;
        let mut cmd = ProcCommand::new(prog);
        for p in prefix {
            cmd.arg(p);
        }
        cmd.arg("-p").arg(&tsconfig);
        let status = cmd
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status();
        match status {
            Ok(s) if s.success() => {
                let node_status = ProcCommand::new("node")
                    .arg(&main_js)
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit())
                    .status();
                match node_status {
                    Ok(s) if s.success() => return ExitCode::SUCCESS,
                    Ok(_) => return ExitCode::FAILURE,
                    Err(e) => {
                        eprintln!(
                            "karnc test: tsc succeeded but `node {}` failed: {e}",
                            main_js.display()
                        );
                        return ExitCode::FAILURE;
                    }
                }
            }
            Ok(_) => {
                eprintln!(
                    "karnc test: tsc reported errors against {}",
                    tsconfig.display()
                );
                return ExitCode::FAILURE;
            }
            Err(_) => {
                // Couldn't launch; try the next candidate.
                continue;
            }
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
        let status = cmd
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status();
        match status {
            Ok(s) if s.success() => return ExitCode::SUCCESS,
            Ok(_) => return ExitCode::FAILURE,
            Err(_) => continue,
        }
    }

    let _ = tsc_attempted;
    eprintln!(
        "karnc test: requires either `tsc` (with Node.js) or `tsx` on PATH. \
         Install one of:\n  - `npm install -g typescript` (provides tsc; requires Node.js to run output)\n  - `npm install -g tsx` (compiles and runs TypeScript in one step)\n  Or run inside a project where `npx tsc` / `npx tsx` resolves.",
    );
    ExitCode::FAILURE
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
    use karnc::fmt::{FormatOptions, format_source};
    let opts = FormatOptions::default();
    if inputs.is_empty() {
        eprintln!("karnc fmt: no input files (pass file paths or `-` for stdin)");
        return ExitCode::FAILURE;
    }
    let mut had_diff = false;
    let mut had_error = false;
    for input in &inputs {
        if input.as_os_str() == "-" {
            use std::io::Read;
            let mut source = String::new();
            if let Err(e) = std::io::stdin().read_to_string(&mut source) {
                eprintln!("karnc fmt: read from stdin: {e}");
                return ExitCode::FAILURE;
            }
            match format_source(&source, &opts) {
                Ok(formatted) => print!("{formatted}"),
                Err(e) => {
                    karnc::print_errors(&e.errors, &source, "<stdin>");
                    return ExitCode::FAILURE;
                }
            }
            continue;
        }
        let source = match std::fs::read_to_string(input) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("karnc fmt: read `{}`: {e}", input.display());
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
                            "karnc fmt: {} is not canonically formatted",
                            input.display()
                        );
                        had_diff = true;
                    }
                } else if formatted != source
                    && let Err(e) = std::fs::write(input, formatted)
                {
                    eprintln!("karnc fmt: write `{}`: {e}", input.display());
                    had_error = true;
                }
            }
            Err(e) => {
                karnc::print_errors(&e.errors, &source, &filename);
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
    platform: karnc::Platform,
) -> ExitCode {
    if input.is_dir() {
        // Multi-file project compile.
        match karnc::compile_project_with_platform(&input, target, platform) {
            Ok(out) => {
                for file in &out.files {
                    let target = output.join(&file.output_path);
                    if let Some(parent) = target.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    if let Err(e) = std::fs::write(&target, &file.typescript) {
                        eprintln!("karnc: could not write `{}`: {e}", target.display());
                        return ExitCode::FAILURE;
                    }
                }
                ExitCode::SUCCESS
            }
            Err(errors) => {
                karnc::print_project_errors(&input, &errors);
                ExitCode::FAILURE
            }
        }
    } else {
        let source = match std::fs::read_to_string(&input) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("karnc: could not read `{}`: {e}", input.display());
                return ExitCode::FAILURE;
            }
        };
        let filename = input.display().to_string();
        match karnc::compile(&source, &filename) {
            Ok(ts) => {
                if let Some(parent) = output.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Err(e) = std::fs::write(&output, ts) {
                    eprintln!("karnc: could not write `{}`: {e}", output.display());
                    return ExitCode::FAILURE;
                }
                ExitCode::SUCCESS
            }
            Err(errors) => {
                karnc::print_errors(&errors, &source, &filename);
                ExitCode::FAILURE
            }
        }
    }
}

fn run_check(input: PathBuf) -> ExitCode {
    if input.is_dir() {
        match karnc::compile_project(&input) {
            Ok(_) => ExitCode::SUCCESS,
            Err(errors) => {
                karnc::print_project_errors(&input, &errors);
                ExitCode::FAILURE
            }
        }
    } else {
        let source = match std::fs::read_to_string(&input) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("karnc: could not read `{}`: {e}", input.display());
                return ExitCode::FAILURE;
            }
        };
        let filename = input.display().to_string();
        match karnc::compile(&source, &filename) {
            Ok(_) => ExitCode::SUCCESS,
            Err(errors) => {
                karnc::print_errors(&errors, &source, &filename);
                ExitCode::FAILURE
            }
        }
    }
}
