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

use project::ProjectOutput;

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
        let target = dir.join(&file.output_path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        match &file.source_map {
            // Slice 1 (ADR 0103): a `.bynk`-sourced file gets a sibling `.ts.map`
            // and a `//# sourceMappingURL` trailer. The trailer lives only on the
            // on-disk artefact — `file.typescript` stays trailer-free, so golden
            // comparisons (which read the in-memory string) are unaffected. The map
            // name appends `.map` to the output file name (e.g. `reps.ts.map`).
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
    }
    Ok(())
}
