//! v0.31 (ADR 0064): locals navigation — resolve the local binding under the
//! cursor and all its sites (its definition plus every use that resolves to
//! it), for `references`, go-to-`definition`, and `documentHighlight`.
//!
//! Slice 1 records bindings with scope ranges (not use sites); the use sites
//! are recovered here by lexing the file and keeping the identifier tokens of
//! the binding's name within its scope that resolve back to it (so a shadowing
//! inner binding's uses — and every binding's *def* token — are excluded).
//! Pure over the analysed snapshot, like `index_queries`.

use karnc::lexer::{self, TokenKind};
use karnc::locals::{LocalBinding, binding_at_def, locals_at};
use karnc::span::Span;

/// The identifier-token name covering `offset`, if any.
fn ident_at(text: &str, offset: usize) -> Option<(&str, Span)> {
    let toks = lexer::tokenize(text).ok()?;
    toks.into_iter()
        .find(|t| t.kind == TokenKind::Ident && t.span.start <= offset && offset <= t.span.end)
        .map(|t| (&text[t.span.start..t.span.end], t.span))
}

/// The binding the cursor refers to — whether it sits on the definition name
/// or on a use — within `locals` (a file's bindings).
fn target_at<'a>(
    locals: &'a [LocalBinding],
    text: &str,
    offset: usize,
) -> Option<&'a LocalBinding> {
    let (name, _) = ident_at(text, offset)?;
    binding_at_def(locals, offset)
        .filter(|b| b.name == name)
        .or_else(|| {
            locals_at(locals, offset)
                .into_iter()
                .find(|b| b.name == name)
        })
}

/// All sites of the local under the cursor — its definition first, then every
/// use that resolves to it (shadowing-safe). `None` when the cursor is not on
/// a local.
pub fn local_sites_at(locals: &[LocalBinding], text: &str, offset: usize) -> Option<Vec<Span>> {
    let target = target_at(locals, text, offset)?;
    let toks = lexer::tokenize(text).ok()?;
    let mut sites = vec![target.def_span];
    for t in &toks {
        if t.kind != TokenKind::Ident || text[t.span.start..t.span.end] != target.name {
            continue;
        }
        if t.span == target.def_span {
            continue; // the definition, already added
        }
        // A binding's own def token is not a use of anything.
        if locals.iter().any(|b| b.def_span == t.span) {
            continue;
        }
        if t.span.start < target.scope.start || t.span.end > target.scope.end {
            continue; // outside the binding's scope
        }
        // Does this use resolve to `target` (not a shadowing inner binding)?
        let resolves = locals_at(locals, t.span.start)
            .into_iter()
            .find(|b| b.name == target.name)
            .map(|b| b.def_span);
        if resolves == Some(target.def_span) {
            sites.push(t.span);
        }
    }
    Some(sites)
}

/// The definition site of the local under the cursor, if any.
pub fn local_definition_at(locals: &[LocalBinding], text: &str, offset: usize) -> Option<Span> {
    target_at(locals, text, offset).map(|b| b.def_span)
}

/// Every local-binding occurrence in the file — `(span, is_definition)` — for
/// semantic-token colouring. A token is a definition if it sits on a binding's
/// def span, else a use if it resolves to a local in scope at that point.
pub fn local_token_sites(locals: &[LocalBinding], text: &str) -> Vec<(Span, bool)> {
    let Ok(toks) = lexer::tokenize(text) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for t in &toks {
        if t.kind != TokenKind::Ident {
            continue;
        }
        let name = &text[t.span.start..t.span.end];
        if locals.iter().any(|b| b.def_span == t.span) {
            out.push((t.span, true)); // a binding's def
        } else if locals_at(locals, t.span.start)
            .into_iter()
            .any(|b| b.name == name)
        {
            out.push((t.span, false)); // a use that resolves to a local
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // `fn f(n: Int) -> Int { let x = n  <uses> }` laid out so offsets are easy.
    fn bindings() -> Vec<LocalBinding> {
        // text: see `TEXT`; n: param scope over body, x: let scope after its stmt.
        vec![
            LocalBinding {
                name: "n".into(),
                def_span: Span { start: 5, end: 6 },
                ty: "Int".into(),
                scope: Span { start: 20, end: 60 },
            },
            LocalBinding {
                name: "x".into(),
                def_span: Span { start: 26, end: 27 },
                ty: "Int".into(),
                scope: Span { start: 34, end: 60 },
            },
        ]
    }

    const TEXT: &str = "fn f(n: Int) -> Int { let x = n\n  x + x\n}";
    //                   0         1         2         3
    //                   0123456789012345678901234567890123456789

    #[test]
    fn sites_for_a_use_collect_def_plus_uses() {
        let locals = bindings();
        // Cursor on the first `x` use (offset 36, in `  x + x`).
        let x_use = TEXT.match_indices('x').nth(1).unwrap().0; // first use of x
        let sites = local_sites_at(&locals, TEXT, x_use).expect("on a local");
        assert!(
            sites.contains(&Span { start: 26, end: 27 }),
            "includes def: {sites:?}"
        );
        assert!(sites.len() >= 2, "def + at least one use: {sites:?}");
    }

    #[test]
    fn definition_resolves_from_a_use() {
        let locals = bindings();
        let n_use = TEXT.rfind('n').unwrap(); // the `n` in `let x = n`
        assert_eq!(
            local_definition_at(&locals, TEXT, n_use),
            Some(Span { start: 5, end: 6 })
        );
    }

    #[test]
    fn not_on_a_local_yields_none() {
        let locals = bindings();
        assert!(local_sites_at(&locals, TEXT, 0).is_none()); // on `fn`
    }

    #[test]
    fn token_sites_mark_definitions_and_uses() {
        let sites = local_token_sites(&bindings(), TEXT);
        assert!(
            sites.iter().any(|(_, decl)| *decl),
            "has a definition token"
        );
        assert!(sites.iter().any(|(_, decl)| !*decl), "has a use token");
        // The `x` def is a declaration token.
        assert!(
            sites.contains(&(Span { start: 26, end: 27 }, true)),
            "x def is a declaration: {sites:?}"
        );
    }

    // End-to-end against real checker output — the lexer's token spans must
    // line up with the checker's recorded def spans.
    #[test]
    fn resolves_a_real_local_from_diagnose_project() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../karnc/tests/fixtures/inlay/clean/src");
        let r = karnc::diagnose_project(&root, &std::collections::HashMap::new());
        let file = r
            .files
            .iter()
            .find(|f| f.source_path.to_string_lossy().ends_with("util.karn"))
            .expect("util.karn analysed");
        let text = &file.text;
        let locals = r
            .locals
            .iter()
            .find(|(p, _)| p.to_string_lossy().ends_with("util.karn"))
            .map(|(_, l)| l.clone())
            .expect("util.karn locals");

        // `let total = …` then `total` — cursor on the use resolves to def + use.
        let use_off = text.rfind("total").expect("total use");
        let sites = local_sites_at(&locals, text, use_off).expect("on a local");
        assert!(sites.len() >= 2, "def + use: {sites:?}");
        // The definition is first and is the `let total` name.
        let def = text.find("total").expect("total def");
        assert_eq!(sites[0].start, def, "def first");
    }
}
