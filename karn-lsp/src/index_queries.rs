//! v0.25 (ADR 0053): pure queries over the project binding index.
//!
//! Everything here is a pure function over [`ProjectIndex`] + analysed
//! snapshot texts — the unit-testable core behind `textDocument/references`
//! and `rename`/`prepareRename`. Transport-side handlers in `main.rs` only
//! convert positions and package results.
//!
//! Rename is validated two ways, both correct-by-construction:
//! 1. **Collisions** — apply the edits to an overlay, re-run
//!    `diagnose_project`, refuse if a new diagnostic appears
//!    ([`no_new_diagnostics`]).
//! 2. **Capture/escape** — re-analysis alone misses silent re-binding
//!    (declared fns shadow fn-typed locals in call position), so the
//!    re-built index must equal the pre-index *modulo the rename*
//!    ([`index_unchanged_modulo_rename`]).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use karnc::index::{ProjectIndex, SiteRef, SymbolKey};
use karnc::span::Span;

/// Definition first, then references — the `references` surface.
pub fn sites_for<'a>(
    index: &'a ProjectIndex,
    path: &Path,
    offset: usize,
    include_declaration: bool,
) -> Option<Vec<&'a SiteRef>> {
    let (key, _) = index.symbol_at(path, offset)?;
    let mut sites = index.sites(key);
    if !include_declaration && !sites.is_empty() {
        sites.remove(0); // definition is always first.
    }
    Some(sites)
}

/// The definition site for the symbol at the cursor (an index-backed,
/// binding-correct go-to-definition).
pub fn definition_at<'a>(
    index: &'a ProjectIndex,
    path: &Path,
    offset: usize,
) -> Option<(&'a SymbolKey, &'a SiteRef)> {
    let (key, _) = index.symbol_at(path, offset)?;
    let entry = index.symbols.get(key)?;
    Some((key, entry.def.as_ref()?))
}

/// `prepareRename`: the renameable range under the cursor, or `None` for
/// out-of-scope symbols (locals, methods, fields, op names, unit names) —
/// the request is refused rather than falling through to a partial rename.
pub fn prepare_rename<'a>(
    index: &'a ProjectIndex,
    path: &Path,
    offset: usize,
) -> Option<(&'a SymbolKey, &'a SiteRef)> {
    index.symbol_at(path, offset)
}

/// v0.26 rider (ADR 0055): `workspace/symbol` — every index definition whose
/// name contains the query, case-insensitive (an empty query lists all),
/// sorted by (name, unit) for a stable order.
pub fn workspace_symbols<'a>(
    index: &'a ProjectIndex,
    query: &str,
) -> Vec<(&'a SymbolKey, &'a SiteRef)> {
    let q = query.to_lowercase();
    let mut out: Vec<_> = index
        .symbols
        .iter()
        .filter(|(k, _)| q.is_empty() || k.name.to_lowercase().contains(&q))
        .filter_map(|(k, e)| e.def.as_ref().map(|d| (k, d)))
        .collect();
    out.sort_by(|a, b| (&a.0.name, &a.0.unit).cmp(&(&b.0.name, &b.0.unit)));
    out
}

/// v0.26 rider (ADR 0055): `documentHighlight` — the symbol-at-cursor's
/// occurrences within that same file (the `references` query, file-scoped).
/// The index does not distinguish read from write references, so the LSP
/// layer omits the highlight `kind`.
pub fn document_highlights<'a>(
    index: &'a ProjectIndex,
    path: &Path,
    offset: usize,
) -> Option<Vec<&'a SiteRef>> {
    let sites = sites_for(index, path, offset, true)?;
    Some(sites.into_iter().filter(|s| s.path == path).collect())
}

/// A planned rename: every name-segment edit, grouped per file, spans
/// ascending. The definition site is edited along with every reference.
#[derive(Debug, Clone)]
pub struct RenamePlan {
    pub key: SymbolKey,
    pub new_name: String,
    pub edits: BTreeMap<PathBuf, Vec<Span>>,
}

/// Build the rename plan for the symbol at the cursor. Errors are
/// human-readable strings surfaced as LSP request failures.
pub fn plan_rename(
    index: &ProjectIndex,
    path: &Path,
    offset: usize,
    new_name: &str,
) -> Result<RenamePlan, String> {
    validate_new_name(new_name)?;
    let (key, _) = index.symbol_at(path, offset).ok_or_else(|| {
        "no renameable symbol at the cursor — types, fns, capabilities, services, \
         agents and providers rename; local bindings, methods, record fields, \
         capability ops and unit names are not yet supported"
            .to_string()
    })?;
    if key.name == new_name {
        return Err(format!("`{new_name}` is already the symbol's name"));
    }
    let mut edits: BTreeMap<PathBuf, Vec<Span>> = BTreeMap::new();
    for site in index.sites(key) {
        edits.entry(site.path.clone()).or_default().push(site.span);
    }
    for spans in edits.values_mut() {
        spans.sort();
        spans.dedup();
    }
    Ok(RenamePlan {
        key: key.clone(),
        new_name: new_name.to_string(),
        edits,
    })
}

/// A new name must lex as exactly one identifier (keywords lex as their own
/// token kinds, so they fail this check).
pub fn validate_new_name(name: &str) -> Result<(), String> {
    let err = || format!("`{name}` is not a valid Karn identifier");
    let tokens = karnc::lexer::tokenize(name).map_err(|_| err())?;
    match tokens.as_slice() {
        [t] if matches!(t.kind, karnc::lexer::TokenKind::Ident)
            && t.span.start == 0
            && t.span.end == name.len() =>
        {
            Ok(())
        }
        _ => Err(err()),
    }
}

/// Apply one file's edits (spans ascending) to its snapshot text.
pub fn apply_edits(text: &str, spans: &[Span], new_name: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for s in spans {
        out.push_str(&text[last..s.start]);
        out.push_str(new_name);
        last = s.end;
    }
    out.push_str(&text[last..]);
    out
}

/// The post-edit position of a pre-edit site — rename edits shift spans
/// within edited files. An edited span maps to the new name's span.
pub fn remap_site(site: &SiteRef, plan: &RenamePlan) -> SiteRef {
    let Some(spans) = plan.edits.get(&site.path) else {
        return site.clone();
    };
    let delta = plan.new_name.len() as isize - plan.key.name.len() as isize;
    let shift: isize = spans.iter().filter(|s| s.end <= site.span.start).count() as isize * delta;
    let start = (site.span.start as isize + shift) as usize;
    let end = if spans.binary_search(&site.span).is_ok() {
        start + plan.new_name.len()
    } else {
        (site.span.end as isize + shift) as usize
    };
    SiteRef {
        path: site.path.clone(),
        span: Span::new(start, end),
    }
}

/// Validator (2): the re-built index must equal the pre-index modulo the
/// rename — every other symbol's reference set identical (after remapping
/// shifted spans), the renamed symbol's sites exactly the edited ones.
/// Catches silent re-binding (capture/escape) that produces no diagnostic.
pub fn index_unchanged_modulo_rename(
    pre: &ProjectIndex,
    post: &ProjectIndex,
    plan: &RenamePlan,
) -> bool {
    pre.equals_modulo_rename(post, &plan.key, &plan.new_name, |s| remap_site(s, plan))
}

/// Validator (1): refuse when the edited project carries a diagnostic the
/// original did not — compared as per-(file, category) counts, robust to
/// span shifts. Removals are tolerated; additions refuse.
pub fn no_new_diagnostics(
    pre: &[(PathBuf, String)],
    post: &[(PathBuf, String)],
) -> Result<(), String> {
    let mut budget: HashMap<(&Path, &str), isize> = HashMap::new();
    for (p, c) in pre {
        *budget.entry((p.as_path(), c.as_str())).or_default() += 1;
    }
    for (p, c) in post {
        let n = budget.entry((p.as_path(), c.as_str())).or_default();
        *n -= 1;
        if *n < 0 {
            return Err(format!(
                "rename would introduce `{c}` in {} — refused",
                p.display()
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use karnc::index::{SymbolEntry, SymbolKind};

    fn site(path: &str, start: usize, end: usize) -> SiteRef {
        SiteRef {
            path: PathBuf::from(path),
            span: Span::new(start, end),
        }
    }

    fn key(unit: &str, kind: SymbolKind, name: &str) -> SymbolKey {
        SymbolKey {
            unit: unit.into(),
            kind,
            name: name.into(),
        }
    }

    fn index_with(entries: Vec<(SymbolKey, SiteRef, Vec<SiteRef>)>) -> ProjectIndex {
        let mut index = ProjectIndex::default();
        for (k, def, refs) in entries {
            index.symbols.insert(
                k,
                SymbolEntry {
                    def: Some(def),
                    refs,
                },
            );
        }
        index
    }

    #[test]
    fn new_name_validation() {
        assert!(validate_new_name("Money2").is_ok());
        assert!(validate_new_name("snake_case").is_ok());
        assert!(validate_new_name("fn").is_err(), "keyword");
        assert!(validate_new_name("two words").is_err());
        assert!(validate_new_name("1abc").is_err());
        assert!(validate_new_name("a.b").is_err());
        assert!(validate_new_name("").is_err());
    }

    #[test]
    fn apply_and_remap_agree() {
        // text: "fn helper(x: Int) -> Int { helper(x) }"
        //        3..9 def                  27..33 ref
        let text = "fn helper(x: Int) -> Int { helper(x) }";
        let k = key("demo.a", SymbolKind::Fn, "helper");
        let index = index_with(vec![(
            k.clone(),
            site("a.karn", 3, 9),
            vec![site("a.karn", 27, 33)],
        )]);
        let plan = plan_rename(&index, Path::new("a.karn"), 4, "do_it").unwrap();
        let edited = apply_edits(text, &plan.edits[Path::new("a.karn")], "do_it");
        assert_eq!(edited, "fn do_it(x: Int) -> Int { do_it(x) }");

        // Remap maps both old sites onto the new spellings.
        for (old, expected) in [
            (site("a.karn", 3, 9), "do_it"),
            (site("a.karn", 27, 33), "do_it"),
        ] {
            let new = remap_site(&old, &plan);
            assert_eq!(&edited[new.span.range()], expected);
        }
        // An unedited later site shifts by the accumulated delta.
        let unrelated = site("a.karn", 34, 35); // `x` argument
        let new = remap_site(&unrelated, &plan);
        assert_eq!(&edited[new.span.range()], "x");
    }

    #[test]
    fn references_listing_orders_definition_first() {
        let k = key("demo.a", SymbolKind::Type, "Money");
        let index = index_with(vec![(
            k,
            site("a.karn", 5, 10),
            vec![site("b.karn", 1, 6), site("a.karn", 20, 25)],
        )]);
        let all = sites_for(&index, Path::new("b.karn"), 3, true).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].path, PathBuf::from("a.karn"));
        assert_eq!(all[0].span, Span::new(5, 10));
        let without_decl = sites_for(&index, Path::new("b.karn"), 3, false).unwrap();
        assert_eq!(without_decl.len(), 2);
    }

    #[test]
    fn rename_refuses_unindexed_positions_and_same_name() {
        let k = key("demo.a", SymbolKind::Fn, "helper");
        let index = index_with(vec![(k, site("a.karn", 3, 9), vec![])]);
        assert!(plan_rename(&index, Path::new("a.karn"), 100, "x").is_err());
        assert!(plan_rename(&index, Path::new("a.karn"), 4, "helper").is_err());
    }

    #[test]
    fn index_equality_detects_escape() {
        // Pre: `helper` has no references; some other symbol unchanged.
        let helper = key("demo.a", SymbolKind::Fn, "helper");
        let money = key("demo.a", SymbolKind::Type, "Money");
        let pre = index_with(vec![
            (helper.clone(), site("a.karn", 3, 9), vec![]),
            (money.clone(), site("a.karn", 50, 55), vec![]),
        ]);
        let plan = plan_rename(&pre, Path::new("a.karn"), 4, "shadow").unwrap();

        // Post (honest): def renamed in place, still no refs.
        let honest = index_with(vec![
            (
                key("demo.a", SymbolKind::Fn, "shadow"),
                site("a.karn", 3, 9),
                vec![],
            ),
            (money.clone(), site("a.karn", 50, 55), vec![]),
        ]);
        assert!(index_unchanged_modulo_rename(&pre, &honest, &plan));

        // Post (escape): a site that used to bind elsewhere now resolves to
        // the renamed symbol — an extra reference appears.
        let escape = index_with(vec![
            (
                key("demo.a", SymbolKind::Fn, "shadow"),
                site("a.karn", 3, 9),
                vec![site("a.karn", 70, 76)],
            ),
            (money, site("a.karn", 50, 55), vec![]),
        ]);
        assert!(!index_unchanged_modulo_rename(&pre, &escape, &plan));
    }

    #[test]
    fn workspace_symbols_filters_and_orders() {
        let index = index_with(vec![
            (
                key("demo.a", SymbolKind::Type, "Money"),
                site("a.karn", 5, 10),
                vec![],
            ),
            (
                key("demo.b", SymbolKind::Fn, "moneyMaker"),
                site("b.karn", 3, 13),
                vec![],
            ),
            (
                key("demo.a", SymbolKind::Fn, "helper"),
                site("a.karn", 40, 46),
                vec![],
            ),
        ]);
        // Case-insensitive substring match.
        let hits = workspace_symbols(&index, "money");
        assert_eq!(
            hits.iter()
                .map(|(k, _)| k.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Money", "moneyMaker"]
        );
        // Empty query lists everything, (name, unit)-ordered.
        assert_eq!(workspace_symbols(&index, "").len(), 3);
        assert!(workspace_symbols(&index, "nothing").is_empty());
    }

    #[test]
    fn document_highlights_are_file_scoped() {
        let k = key("demo.a", SymbolKind::Type, "Money");
        let index = index_with(vec![(
            k,
            site("a.karn", 5, 10),
            vec![site("b.karn", 1, 6), site("a.karn", 20, 25)],
        )]);
        // From a.karn: the definition + the in-file reference, not b.karn's.
        let highlights = document_highlights(&index, Path::new("a.karn"), 7).unwrap();
        assert_eq!(highlights.len(), 2);
        assert!(highlights.iter().all(|s| s.path == Path::new("a.karn")));
        // No symbol at the cursor → None.
        assert!(document_highlights(&index, Path::new("a.karn"), 100).is_none());
    }

    #[test]
    fn diagnostic_budget_allows_removals_refuses_additions() {
        let pre = vec![
            (PathBuf::from("a.karn"), "karn.x".to_string()),
            (PathBuf::from("a.karn"), "karn.x".to_string()),
        ];
        let same = pre.clone();
        assert!(no_new_diagnostics(&pre, &same).is_ok());
        assert!(no_new_diagnostics(&pre, &pre[..1]).is_ok());
        let mut more = pre.clone();
        more.push((PathBuf::from("b.karn"), "karn.resolve.duplicate_fn".into()));
        assert!(no_new_diagnostics(&pre, &more).is_err());
    }
}
