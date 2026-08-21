//! In-process test helpers shared across `bynk-emit`'s `#[cfg(test)]` modules
//! (#57, testing track). Before this existed, driving anything beyond a
//! self-contained single-file commons through the emitter meant going through
//! `bynkc`'s on-disk fixture directories — the only fs-free seam
//! (`project::compile_in_memory`) was private to one test module
//! (`emitter::conditional_runtime_import_tests`), so every other test that
//! needed the project pipeline (cross-context `uses`, multi-file `Split`
//! layouts, …) would have hand-rolled its own copy or fallen back to a disk
//! fixture. Promoted here so it can't happen twice.

/// Compile a self-contained single-file commons and return its TypeScript.
/// Panics with the source on a compile failure — a test fixture that doesn't
/// compile is a bug in the test, not a case to assert on.
pub(crate) fn emit_source(src: &str) -> String {
    crate::compile(src, "t.bynk").expect("fixture should compile")
}

/// Emit a `commons app.bundle` wrapping `body`, pre-declared with `uses
/// bynk.locale`/`uses bynk.locale.types`, and return its own module's
/// TypeScript out of the project pipeline.
///
/// A bundle needing `uses bynk.locale` can't go through [`emit_source`] — the
/// single-file path rejects `uses` by construction — so this drives
/// [`crate::project::compile_in_memory`] instead and picks the user's own
/// unit out of the returned module graph.
pub(crate) fn emit_bundle(body: &str) -> String {
    let src = format!("commons app.bundle\n\nuses bynk.locale\nuses bynk.locale.types\n\n{body}");
    let out = match crate::project::compile_in_memory(
        &src,
        crate::project::BuildTarget::Bundle,
        Default::default(),
    ) {
        Ok(out) => out,
        Err(_) => panic!("bundle fixture should compile:\n{src}"),
    };
    out.artefacts
        .docs
        .iter()
        .find(|(path, _)| path.ends_with("bundle.ts"))
        .map(|(_, doc)| doc.text())
        .expect("the bundle's own module should be in the output")
}
