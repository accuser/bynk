//! Workstream 0 (docs-reorg proposal): the book's **current-version banners**
//! must agree with the released version — they drifted six ways (v0.20/v0.25/
//! v0.26/…) because `bump-version.sh` never touched `docs/`. This pins each
//! banner to the crate's major.minor (patches are non-language, so the banners
//! track `MAJOR.MINOR`). `bump-version.sh` now rewrites them; this test fails
//! CI if a future bump skips a page.
//!
//! Only the "current version" banners are checked — *not* the historical
//! "introduced in vX" feature markers in `spec/*` and the roadmap, which are
//! correct as written and must not move.

use std::fs;
use std::path::PathBuf;

fn docs_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../docs/src")
}

/// The crate version's `MAJOR.MINOR` — the granularity the book documents.
fn major_minor() -> String {
    let v = env!("CARGO_PKG_VERSION");
    let mut it = v.split('.');
    format!("{}.{}", it.next().unwrap(), it.next().unwrap())
}

#[test]
fn current_version_banners_agree_with_the_release() {
    let mm = major_minor();
    // (page, the banner phrase that must carry the current version)
    let banners = [
        ("introduction.md", format!("currently v{mm}")),
        ("tooling/index.md", format!("currently v{mm}")),
        (
            "about/versioning-and-roadmap.md",
            format!("written against v{mm}"),
        ),
        ("spec/scope.md", format!("current version, v{mm}")),
        (
            "spec/appendix-version-history.md",
            format!("current version, v{mm}"),
        ),
        ("spec/index.md", format!("current version, v{mm}")),
        (
            "reference/changelog.md",
            format!("written against **v{mm}**"),
        ),
    ];
    let mut stale = Vec::new();
    for (page, phrase) in &banners {
        let text = fs::read_to_string(docs_src().join(page))
            .unwrap_or_else(|e| panic!("read docs/src/{page}: {e}"));
        if !text.contains(phrase) {
            stale.push(format!("  docs/src/{page} — expected `{phrase}`"));
        }
    }
    assert!(
        stale.is_empty(),
        "doc version banner(s) out of date with v{mm} — run `scripts/bump-version.sh {}`:\n{}",
        env!("CARGO_PKG_VERSION"),
        stale.join("\n")
    );
}
