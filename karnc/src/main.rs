//! karnc — the Karn v0 compiler CLI.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "karnc", version, about = "Karn v0 compiler", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Compile a `.karn` file to TypeScript.
    Compile {
        /// Input `.karn` file.
        input: PathBuf,
        /// Output `.ts` file.
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Type-check a `.karn` file without writing output.
    Check {
        /// Input `.karn` file.
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

fn run_check(input: PathBuf) -> ExitCode {
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
