//! Karn v0 compiler library.
//!
//! Compiles a `.karn` commons source file to a TypeScript module.
//!
//! Pipeline: lex → parse → resolve → check → emit.

pub mod ast;
pub mod checker;
pub mod emitter;
pub mod error;
pub mod lexer;
pub mod parser;
pub mod resolver;
pub mod span;

use std::path::Path;

use ariadne::Source;

pub use error::CompileError;

/// Compile a Karn source string to a TypeScript string.
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

#[allow(dead_code)]
fn _path_unused(_: &Path) {}
