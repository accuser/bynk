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

use std::path::Path;

use bynk_check::{checker, resolver};
use bynk_syntax::{CompileError, lexer, parser};
use project::{CompiledFile, ProjectOutput};

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
    let boundary_types = project::collect_type_decls(commons.items.iter());
    project::check_function_type_boundary_items(
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
    Ok(Compiled {
        ts: emitter::emit(&typed),
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

/// Write a [`ProjectOutput`]'s files under `dir`, creating parent directories as
/// needed. The shared writer behind both `bynkc`'s `compile`/`test` paths and
/// `bynk dev`'s in-process build (slice 7) — so the on-disk result is identical
/// however the build was driven.
pub fn write_output(out: &ProjectOutput, dir: &Path) -> std::io::Result<()> {
    for file in &out.files {
        write_compiled_file(file, dir)?;
    }
    Ok(())
}

/// Write a single [`CompiledFile`] under `dir`, map-aware: a `.bynk`-sourced file
/// gets a sibling `.ts.map` and a `//# sourceMappingURL` trailer (slice 1, ADR
/// 0103); a file with no map is written verbatim. Shared by [`write_output`] and
/// `bynkc test`'s output loops, so every disk-writing path emits maps uniformly
/// (slice 2 — `bynkc test --inspect` runs the emitted `.ts` directly and needs
/// the maps on disk). The trailer lives only on the on-disk artefact; the
/// in-memory `file.typescript` stays trailer-free, so golden comparisons are
/// unaffected. The map name appends `.map` to the output file name.
pub fn write_compiled_file(file: &CompiledFile, dir: &Path) -> std::io::Result<()> {
    let target = dir.join(&file.output_path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match &file.source_map {
        Some(map) => {
            let map_name = match target.file_name() {
                Some(n) => format!("{}.map", n.to_string_lossy()),
                None => "module.ts.map".to_string(),
            };
            let map_path = target.with_file_name(&map_name);
            std::fs::write(&map_path, map)?;
            let with_trailer = format!("{}//# sourceMappingURL={map_name}\n", file.typescript);
            std::fs::write(&target, with_trailer)?;
        }
        None => std::fs::write(&target, &file.typescript)?,
    }
    // Slice 3 (ADR 0105): the debug-metadata sidecar — a `<file>.bynkdbg.json` next
    // to the `.ts`, mapping each emitted handler to its Bynk operation label so the
    // debugger names stack frames in Bynk. A sibling like the `.ts.map`; not bundled
    // into a deployed Worker.
    if let Some(meta) = &file.debug_metadata {
        let meta_name = match target.file_name() {
            Some(n) => format!("{}.bynkdbg.json", n.to_string_lossy()),
            None => "module.ts.bynkdbg.json".to_string(),
        };
        std::fs::write(target.with_file_name(meta_name), meta)?;
    }
    Ok(())
}
