//! Bynk's build orchestration and TypeScript emission — the layer above
//! `bynk-check`.
//!
//! `project` is the build driver: it conducts discovery, the dependency graph,
//! consistency, validation, symbols, and paths, and owns `compile_project`.
//! `emitter` lowers a checked program to TypeScript. Read the crate as "build
//! orchestration + TS emission" — orchestration drives emission.
//!
//! Extracted from `bynkc` as slice 4 of the crate-decomposition track over
//! `bynk-syntax` + `bynk-check`. Behaviour is unchanged; `bynkc` depends on this
//! crate and re-exports its modules so its public API (`compile_project`,
//! `ProjectOutput`, …) and the binary are untouched.

pub mod emitter;
pub mod project;

#[cfg(test)]
pub(crate) mod testkit;

use bynk_check::{checker, resolver};
use bynk_syntax::{CompileError, lexer, parser};

/// A single-file compile that also returns the non-failing warnings produced on
/// success — what a CLI prints (v0.89, ADR 0117). [`compile`] is the
/// warning-discarding convenience over this.
///
/// Lives in `bynk-emit` (slice 7 precedent, alongside [`NODE_MAJOR_FLOOR`]) so
/// both `bynkc` and the `bynk` driver can compile a self-contained single-file
/// commons in-process without depending on each other; `bynkc` re-exports it so
/// `bynkc::compile_with_warnings` and `bynkc::Compiled` are unchanged.
pub struct Compiled {
    pub ts: String,
    pub warnings: Vec<CompileError>,
}

/// Compile a single Bynk source string to a TypeScript string.
///
/// Parses the input as a self-contained, single-file commons with no `uses`
/// against other commons. Use [`project::compile_project`] for multi-file
/// projects or for any source that declares `uses`. `filename` is used only for
/// diagnostic rendering.
pub fn compile(source: &str, filename: &str) -> Result<String, Vec<CompileError>> {
    compile_with_warnings(source, filename).map(|c| c.ts)
}

/// The warning-preserving single-file compile behind [`compile`]. See [`Compiled`].
pub fn compile_with_warnings(source: &str, _filename: &str) -> Result<Compiled, Vec<CompileError>> {
    let tokens = lexer::tokenize(source).map_err(|e| vec![e])?;
    // ADR 0117: parse-time warnings (orphan doc blocks) ride alongside the
    // AST — they surface with the build's warnings instead of failing it.
    let (commons, mut warnings) = parser::parse_with_warnings(&tokens, source)?;
    // v0.20a: function types are confined to non-boundary positions — the same
    // rule the project path applies.
    let mut boundary_errors = Vec::new();
    let boundary_types = bynk_check::project_model::collect_type_decls(commons.items.iter());
    bynk_check::project_model::check_function_type_boundary_items(
        &commons.items,
        &boundary_types,
        &mut boundary_errors,
    );
    if !boundary_errors.is_empty() {
        return Err(boundary_errors);
    }
    let resolved = resolver::resolve(commons)?;
    let typed = checker::check(resolved)?;
    warnings.extend(typed.warnings.clone());
    // T3.7 (R3.10): `check` already gated on error-severity diagnostics, so
    // `typed.warnings` — the only diagnostics left riding along with it — can
    // never contain one; `certify` re-asserts that structurally rather than
    // trusting the caller not to skip it.
    let program = checker::certify(typed, warnings.clone()).unwrap_or_else(|_| {
        panic!("bynk internal error: check() already gated on error-severity diagnostics")
    });
    Ok(Compiled {
        ts: emitter::emit(&program),
        warnings,
    })
}

/// Minimum supported Node.js **major** version for the `node` platform binding
/// and for running Bynk's emitted TypeScript.
///
/// Single source of truth for the Node floor: the emitted code targets it, the
/// `bynk` driver's `doctor` command compares a detected `node` against it, and
/// `bynkc`'s CLI re-exports it rather than restating the number. Lives in
/// `bynk-emit` (which emits the TS that runs on Node) so both binaries share one
/// definition (slice 7; was a `bynkc` const before the driver dropped that dep).
pub const NODE_MAJOR_FLOOR: u32 = 18;

// `write_output`/`write_compiled_file` moved to `bynk-driver` (#1047, R2.3/
// T0.7 residue): every caller was already at driver level, so this crate
// never needed direct filesystem access for it — the pure move closes it
// out of this crate's `fs_below_driver` count. See `bynk-driver::output`.
