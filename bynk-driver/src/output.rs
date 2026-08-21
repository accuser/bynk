//! Writing a compiled project's output to disk.
//!
//! Moved down from `bynk-emit` (#1047, R2.3/T0.7 residue): every caller was
//! already at driver level (`bynkc`'s CLI/test paths, `bynk dev`'s in-process
//! build) — `bynk-emit` never called this itself, so relocating it here is a
//! pure move, not a design change. `bynk-emit` stays a pure, in-memory
//! library; disk writes are the driver's job, as R2.3 says they should be.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bynk_emit::project::{Document, ProjectOutput, sibling_path};

/// Write a [`ProjectOutput`]'s artefacts under `dir`, creating parent
/// directories as needed. The shared writer behind both `bynkc`'s
/// `compile`/`test` paths and `bynk dev`'s in-process build (slice 7) — so
/// the on-disk result is identical however the build was driven.
///
/// Reconciles `dir` against `out.artefacts.docs` first: a `.ts`/`.js`/
/// `.map`/`.json`/`.toml` file already on disk that no longer corresponds to
/// anything in `out.artefacts.docs` is deleted, along with any directory
/// that becomes empty as a result — otherwise a deleted `.bynk` unit's
/// emitted `.ts` lingers on disk, still type-checked by the emitted
/// `tsconfig.json`'s `include: **/*.ts`, so `tsc` fails against a module the
/// current project no longer has. `node_modules` and dotfile directories
/// (`.git`, an npm-installed tree under the output root) are never
/// descended into — this reconciles the compiler's own output, not whatever
/// else happens to live alongside it.
pub fn write_output(out: &ProjectOutput, dir: &Path) -> std::io::Result<()> {
    prune_stale_output(out, dir)?;
    for (path, doc) in &out.artefacts.docs {
        write_document(path, doc, &out.artefacts.docs, dir)?;
    }
    Ok(())
}

/// Write one [`Document`] under `dir` — the real, typed write boundary
/// (P7.6, #1309, Decision C: one place derives the sibling-path relationship
/// instead of two independently-maintained ones). Shared by [`write_output`]
/// and `bynk-driver::test_runner`'s own output loop (`bynkc test`), so every
/// disk-writing path emits maps uniformly (slice 2 — `bynkc test --inspect`
/// runs the emitted `.ts` directly and needs the maps on disk).
///
/// A `Ts` document is printed through the one printer that owns a character
/// (R7.3); its own `Printed::source_map` is discarded, not written — the
/// real map, when one exists, is already present in `docs` as its own
/// `SourceMap` entry at this path's `.map` sibling ([`sibling_path`]), split
/// out at construction (`bynk-emit::project::build_output`'s own tail).
/// That sibling's presence, not `Printed::source_map`, is what decides
/// whether the `.ts`/`.js` file gets a `//# sourceMappingURL=` trailer — the
/// trailer lives only on the on-disk artefact, not on `docs`' own in-memory
/// text, so golden comparisons are unaffected.
pub fn write_document(
    path: &Path,
    doc: &Document,
    docs: &BTreeMap<PathBuf, Document>,
    dir: &Path,
) -> std::io::Result<()> {
    let target = dir.join(path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mapped_text = match doc {
        Document::Ts(program) => {
            Some(bynk_ts::print(program, "", "", &path.to_string_lossy()).text)
        }
        Document::Js(s) => Some(s.clone()),
        _ => None,
    };
    if let Some(text) = mapped_text {
        match docs.get(&sibling_path(path, "map")) {
            Some(_) => {
                let map_name = match target.file_name() {
                    Some(n) => format!("{}.map", n.to_string_lossy()),
                    None => "module.ts.map".to_string(),
                };
                let with_trailer = format!("{text}//# sourceMappingURL={map_name}\n");
                std::fs::write(&target, with_trailer)?;
            }
            None => std::fs::write(&target, &text)?,
        }
        return Ok(());
    }
    match doc {
        Document::Toml(t) => {
            std::fs::write(
                &target,
                bynk_emit::emitter::toml_doc::print_toml_document(t),
            )?;
        }
        Document::Json(s) | Document::SourceMap(s) | Document::DebugSidecar(s) => {
            std::fs::write(&target, s)?;
        }
        Document::Ts(_) | Document::Js(_) => unreachable!("handled above"),
    }
    Ok(())
}

/// The project-relative paths [`write_output`] will have written once this
/// `ProjectOutput` lands on disk — exactly `out.artefacts.docs`'s own keys,
/// since `bynk-emit::project::build_output`'s own tail already splits every
/// `.map`/`.bynkdbg.json` sidecar into its own entry there (Decision C).
fn expected_output_paths(out: &ProjectOutput) -> std::collections::HashSet<PathBuf> {
    out.artefacts.docs.keys().cloned().collect()
}

/// Extensions the compiler ever writes under a build-output directory — the
/// set [`write_output`]'s reconciliation is allowed to prune. Kept narrow so a
/// directory the caller points `write_output` at can still carry other files
/// unrelated to a `.bynk` build without those being swept up.
fn is_prunable_output_extension(ext: &str) -> bool {
    matches!(ext, "ts" | "js" | "map" | "json" | "toml")
}

fn prune_stale_output(out: &ProjectOutput, dir: &Path) -> std::io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    let expected = expected_output_paths(out);
    let mut dirs_visited = Vec::new();
    prune_stale_output_dir(dir, dir, &expected, &mut dirs_visited)?;
    // Remove directories left empty by the file removals above, deepest first
    // (a parent only empties out once its children are gone). `remove_dir` is
    // a no-op error (ignored) on anything still non-empty — e.g. a directory
    // that held only unrelated files to begin with.
    dirs_visited.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
    for d in dirs_visited {
        let _ = std::fs::remove_dir(&d);
    }
    Ok(())
}

fn prune_stale_output_dir(
    root: &Path,
    dir: &Path,
    expected: &std::collections::HashSet<std::path::PathBuf>,
    dirs_visited: &mut Vec<std::path::PathBuf>,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            let is_own_cache = path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n == "node_modules" || n.starts_with('.'));
            if is_own_cache {
                continue;
            }
            prune_stale_output_dir(root, &path, expected, dirs_visited)?;
            dirs_visited.push(path);
        } else if file_type.is_file() {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let rel = path.strip_prefix(root).unwrap_or(&path);
            if is_prunable_output_extension(ext) && !expected.contains(rel) {
                std::fs::remove_file(&path)?;
            }
        }
    }
    Ok(())
}
