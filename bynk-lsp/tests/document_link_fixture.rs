//! #848 — `textDocument/documentLink`'s intra-doc-link half, end to end.
//!
//! `document_link` itself was, before this, covered only by a
//! capability-advertisement test (`advertises_document_links` in `src/lib.rs`)
//! — no end-to-end fixture test existed at all, for either the pre-existing
//! `uses`/`consumes` links or the new intra-doc ones. This closes that gap for
//! the intra-doc half.
//!
//! `Backend::document_link` is transport (state locks, live-buffer read); the
//! part under test here is the pure resolution its body wires together —
//! `symbols::own_declaration_name` + `symbols::doc_link_spans` +
//! `index_queries::resolve_doc_link` — run directly against a real
//! `bynk_ide::diagnose_project` analysis of `examples/rate-limiter`, the way
//! `hover_references.rs` establishes for hover. That fixture's doc comments
//! (see `src/ratelimit.bynk`/`src/window.bynk`) exercise every decided case in
//! one project: a bare local name, a local dotted field (code-span form), a
//! cross-unit `uses` hit, an unresolved first-party name, and an explicit URL.
//!
//! Excluded from the published crate (`Cargo.toml`) — reads `../examples`,
//! outside this crate's package, same as `hover_references.rs`.

use std::collections::HashMap;
use std::path::Path;

use bynk_check::index::SiteRef;
use bynk_ide::ProjectDiagnostics;
use bynk_lsp::{index_queries, symbols};

/// The analysed `examples/rate-limiter` project.
fn rate_limiter() -> ProjectDiagnostics {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/rate-limiter/src");
    bynk_ide::diagnose_project(&root, &HashMap::new())
}

fn file_text(r: &ProjectDiagnostics, name: &str) -> String {
    r.files
        .iter()
        .find(|f| f.source_path.to_string_lossy().ends_with(name))
        .unwrap_or_else(|| panic!("{name} analysed"))
        .text
        .clone()
}

/// Every intra-doc link `document_link` would emit for `file`'s own doc
/// comments — `(candidate name, resolved definition)`, resolved exactly the
/// way the handler resolves them (`own_declaration_name` for the owning
/// unit, `doc_link_spans` for the candidates, `resolve_doc_link` for each).
fn doc_links_in(r: &ProjectDiagnostics, file: &str) -> Vec<(String, Option<SiteRef>)> {
    let text = file_text(r, file);
    let (owner_unit, _) =
        symbols::own_declaration_name(&text).expect("a context/commons file has an own name");
    symbols::doc_link_spans(&text)
        .into_iter()
        .map(|(name, _span)| {
            let def = index_queries::resolve_doc_link(&r.index, &r.doc_scope, &owner_unit, &name)
                .cloned();
            (name, def)
        })
        .collect()
}

#[test]
fn bare_local_name_resolves_to_its_own_file() {
    let r = rate_limiter();
    let links = doc_links_in(&r, "ratelimit.bynk");
    let (_, def) = links
        .iter()
        .find(|(n, _)| n == "Limiter")
        .expect("`[Limiter]` is a candidate in ratelimit.bynk");
    let def = def.as_ref().expect("`Limiter` resolves");
    assert!(
        def.path.to_string_lossy().ends_with("ratelimit.bynk"),
        "{def:?}"
    );
}

#[test]
fn bare_local_service_name_resolves() {
    let r = rate_limiter();
    let links = doc_links_in(&r, "ratelimit.bynk");
    let (_, def) = links
        .iter()
        .find(|(n, _)| n == "api")
        .expect("`[api]` is a candidate in ratelimit.bynk");
    let def = def.as_ref().expect("`api` resolves");
    assert!(
        def.path.to_string_lossy().ends_with("ratelimit.bynk"),
        "{def:?}"
    );
}

#[test]
fn dotted_local_field_resolves_via_the_code_span_form() {
    let r = rate_limiter();
    let links = doc_links_in(&r, "ratelimit.bynk");
    let (_, def) = links
        .iter()
        .find(|(n, _)| n == "RateView.remaining")
        .expect("`[`RateView.remaining`]` is a candidate in ratelimit.bynk");
    let def = def.as_ref().expect("`RateView.remaining` resolves");
    assert!(
        def.path.to_string_lossy().ends_with("ratelimit.bynk"),
        "{def:?}"
    );
}

#[test]
fn bare_name_resolves_cross_unit_via_uses() {
    let r = rate_limiter();
    let links = doc_links_in(&r, "ratelimit.bynk");
    let (_, def) = links
        .iter()
        .find(|(n, _)| n == "decide")
        .expect("`[decide]` is a candidate in ratelimit.bynk");
    let def = def
        .as_ref()
        .expect("`decide` resolves, via `ratelimit uses window`");
    assert!(
        def.path.to_string_lossy().ends_with("window.bynk"),
        "{def:?}"
    );
}

#[test]
fn first_party_capability_op_is_unresolved() {
    let r = rate_limiter();
    let links = doc_links_in(&r, "ratelimit.bynk");
    let (_, def) = links
        .iter()
        .find(|(n, _)| n == "Clock.now")
        .expect("`[Clock.now]` is a candidate in ratelimit.bynk");
    assert!(
        def.is_none(),
        "a first-party capability op has no on-disk location to link to: {def:?}"
    );
}

#[test]
fn explicit_url_is_never_a_candidate() {
    let r = rate_limiter();
    let links = doc_links_in(&r, "window.bynk");
    assert!(
        links.iter().all(|(n, _)| n != "fixed window rate limiting"),
        "an explicit `[text](url)` link must never be scanned as a doc-link candidate: {links:?}"
    );
}
