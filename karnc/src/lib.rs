//! Karn v0.3 compiler library.
//!
//! Compiles `.karn` commons source into TypeScript modules.
//!
//! Pipeline: lex → parse → resolve → check → emit.
//!
//! v0.3 introduces multi-file commons and the `uses` mechanism. A "project"
//! is a directory containing one or more commons; a commons is either a
//! single `.karn` file or a directory of `.karn` files that share a
//! `commons name` header. See [`compile_project`].
//!
//! The single-string entrypoint [`compile`] remains for v0–v0.2 fixtures
//! and any single-file commons that does not declare `uses` against another
//! commons.

pub mod ast;
pub mod checker;
pub mod emitter;
pub mod error;
pub mod lexer;
pub mod parser;
pub mod project;
pub mod resolver;
pub mod span;

use std::path::Path;

use ariadne::Source;

pub use error::CompileError;
pub use project::{CompiledFile, ProjectOutput, compile_project};

/// Compile a single Karn source string to a TypeScript string.
///
/// This entry point parses the input as a self-contained, single-file commons
/// with no `uses` against other commons. Use [`compile_project`] for
/// multi-file projects or for any source that declares `uses`.
///
/// `filename` is used only for diagnostic rendering.
pub fn compile(source: &str, _filename: &str) -> Result<String, Vec<CompileError>> {
    let tokens = lexer::tokenize(source).map_err(|e| vec![e])?;
    let commons = parser::parse(&tokens, source)?;
    let resolved = resolver::resolve(commons)?;
    let typed = checker::check(resolved)?;
    Ok(emitter::emit(&typed))
}

/// Render a list of compile errors to a string (for tests) using the given filename
/// as the diagnostic source label.
pub fn render_errors(errors: &[CompileError], source: &str, filename: &str) -> String {
    let mut out = Vec::new();
    let mut cache = (filename, Source::from(source));
    for err in errors {
        err.report(filename)
            .write(&mut cache, &mut out)
            .expect("write to Vec<u8> cannot fail");
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Render to stderr with color, used by the CLI.
pub fn print_errors(errors: &[CompileError], source: &str, filename: &str) {
    let mut cache = (filename, Source::from(source));
    for err in errors {
        let _ = err.report(filename).eprint(&mut cache);
    }
}

/// Render project-level errors. Each error's span refers to *some* file in
/// the project, but we don't know which without a span-to-file index. For
/// now, print errors as plain text since they aren't tied to a single file.
pub fn print_project_errors(root: &Path, errors: &[CompileError]) {
    let _ = root;
    for err in errors {
        eprintln!("[{}] {}", err.category, err.message);
        for note in &err.notes {
            eprintln!("  note: {note}");
        }
    }
}

/// Render project-level errors to a string (for test assertion).
pub fn render_project_errors(errors: &[CompileError]) -> String {
    let mut out = String::new();
    for err in errors {
        out.push_str(&format!("[{}] {}\n", err.category, err.message));
        for note in &err.notes {
            out.push_str(&format!("  note: {note}\n"));
        }
    }
    out
}

#[allow(dead_code)]
fn _path_unused(_: &Path) {}
