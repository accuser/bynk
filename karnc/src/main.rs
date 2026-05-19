//! karnc — the Karn v0.3 compiler CLI.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "karnc", version, about = "Karn v0.3 compiler", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Compile a `.karn` file (single-file commons) to a TypeScript file,
    /// or a directory project to a tree of TypeScript files mirroring the
    /// source layout.
    Compile {
        /// Input `.karn` file, or directory project root.
        input: PathBuf,
        /// Output `.ts` file (for single-file input) or output root
        /// directory (for project input).
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Type-check a `.karn` file or project without writing output.
    Check {
        /// Input `.karn` file or project root.
        input: PathBuf,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Compile { input, output } => run_compile(input, output),
        Command::Check { input } => run_check(input),
    }
}

fn run_compile(input: PathBuf, output: PathBuf) -> ExitCode {
    if input.is_dir() {
        // Multi-file project compile.
        match karnc::compile_project(&input) {
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
