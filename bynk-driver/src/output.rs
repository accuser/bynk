//! Writing a compiled project's output to disk.
//!
//! Moved down from `bynk-emit` (#1047, R2.3/T0.7 residue): every caller was
//! already at driver level (`bynkc`'s CLI/test paths, `bynk dev`'s in-process
//! build) — `bynk-emit` never called this itself, so relocating it here is a
//! pure move, not a design change. `bynk-emit` stays a pure, in-memory
//! library; disk writes are the driver's job, as R2.3 says they should be.

use std::path::Path;

use bynk_emit::project::{CompiledFile, ProjectOutput};

/// Write a [`ProjectOutput`]'s files under `dir`, creating parent directories as
/// needed. The shared writer behind both `bynkc`'s `compile`/`test` paths and
/// `bynk dev`'s in-process build (slice 7) — so the on-disk result is identical
/// however the build was driven.
///
/// Reconciles `dir` against `out.files` first: a `.ts`/`.js`/`.map`/`.json`/
/// `.toml` file already on disk that no longer corresponds to anything in
/// `out.files` (or one of its `.map`/`.bynkdbg.json` sidecars) is deleted,
/// along with any directory that becomes empty as a result — otherwise a
/// deleted `.bynk` unit's emitted `.ts` lingers on disk, still type-checked by
/// the emitted `tsconfig.json`'s `include: **/*.ts`, so `tsc` fails against a
/// module the current project no longer has. `node_modules` and dotfile
/// directories (`.git`, an npm-installed tree under the output root) are
/// never descended into — this reconciles the compiler's own output, not
/// whatever else happens to live alongside it.
pub fn write_output(out: &ProjectOutput, dir: &Path) -> std::io::Result<()> {
    prune_stale_output(out, dir)?;
    for file in &out.files {
        write_compiled_file(file, dir)?;
    }
    Ok(())
}

/// The project-relative paths [`write_output`] will have written once this
/// `ProjectOutput` lands on disk — each `CompiledFile::output_path` plus its
/// `.map` / `.bynkdbg.json` sidecars, named exactly as [`write_compiled_file`]
/// names them.
fn expected_output_paths(out: &ProjectOutput) -> std::collections::HashSet<std::path::PathBuf> {
    let mut expected = std::collections::HashSet::new();
    for file in &out.files {
        expected.insert(file.output_path.clone());
        let Some(name) = file.output_path.file_name() else {
            continue;
        };
        if file.source_map.is_some() {
            let map_name = format!("{}.map", name.to_string_lossy());
            expected.insert(file.output_path.with_file_name(map_name));
        }
        if file.debug_metadata.is_some() {
            let meta_name = format!("{}.bynkdbg.json", name.to_string_lossy());
            expected.insert(file.output_path.with_file_name(meta_name));
        }
    }
    expected
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
