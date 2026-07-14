//! #611 (ADR 0190): hover's resolution ladder, as one pure function.
//!
//! `Backend::hover` is transport — it resolves the cursor position, gathers the
//! round's tables and the live buffer, and packages the result as an LSP
//! `Hover`. The *order* the rungs are tried in is the behaviour: #611's gap B was
//! a fall-through bug, where a rung that resolved the offset correctly but
//! rendered nothing let a later, name-matching rung answer instead — a
//! confidently wrong hover. That order lives here, once, so a test can **pin** it
//! rather than replicate it (a replica agrees with the original only until
//! someone reorders one of them).
//!
//! Two text sources, deliberately: the index rungs read the round's **analysed
//! snapshot** (the tables' spans index into it), while the lexical rungs read the
//! **live buffer**, which is what makes hover work mid-edit. They diverge while
//! the user types, so the ladder keeps them distinct rather than assuming one.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bynk_syntax::span::Span;
use tower_lsp::lsp_types::Url;

/// The analysed round's tables, positioned at the cursor.
pub(crate) struct HoverAnalysis<'a> {
    pub index: &'a bynk_check::index::ProjectIndex,
    /// Project-relative path → the analysed text.
    pub snapshots: &'a HashMap<PathBuf, String>,
    pub locals: &'a bynk_check::locals::FileLocals,
    pub expr_types: &'a bynk_check::expr_types::FileExprTypes,
    /// The cursor's file (project-relative) and its offset **into the snapshot**.
    pub rel: &'a Path,
    pub offset: usize,
}

/// Everything the ladder reads.
pub(crate) struct HoverInput<'a> {
    /// The analysed round; `None` when the file is outside it — the lexical
    /// rungs still answer from the live buffer.
    pub analysis: Option<HoverAnalysis<'a>>,
    /// The **live** buffer and the cursor's offset into it; `None` when the
    /// document is not open — the index rungs still answer from the snapshot.
    pub doc: Option<(&'a str, usize)>,
    pub uri: &'a Url,
    pub src_root: Option<&'a Path>,
}

/// The hover Markdown for the cursor, or `None` when no rung resolves it.
///
/// The rung order is the contract; see the module doc. Each rung is tried in
/// turn and the first `Some` wins.
pub(crate) fn hover_content(input: &HoverInput<'_>) -> Option<String> {
    if let Some(a) = &input.analysis {
        // 1. v0.25: binding-index path — a resolved symbol reference, described
        //    from its *defining* file (names are unique per file, so the per-file
        //    lookup is exact). Binding-correct: a duplicate name in another unit
        //    describes the bound declaration, not the first name match.
        //
        //    v0.166 (ADR 0191): every `SymbolKind` now has a renderer arm, so a
        //    resolved key is answered here rather than falling through to a name
        //    match below. The `Some` guard remains — it is what a new kind added
        //    without an arm would fall through, silently, which is how
        //    `Method`/`CapabilityOp` came to render the wrong declaration.
        if let Some((key, def)) = crate::index_queries::definition_at(a.index, a.rel, a.offset)
            && let Some(def_text) = a.snapshots.get(&def.path)
            && let Some(content) = crate::symbols::describe_symbol(def_text, &key.name)
        {
            return Some(content);
        }
        // 2. #611 (gap C): a `store` field's operation (`items.put(…)`) — a
        //    structural match on the enclosing agent's declared field, so it
        //    outranks the locals rung below, which guesses by name in scope.
        //    Takes `locals` to honour the checker's by-provenance dispatch: a
        //    local shadowing the field makes this an ordinary value method.
        if let Some(text) = a.snapshots.get(a.rel)
            && let Some(content) = crate::symbols::describe_store_op_at(
                text,
                a.offset,
                a.locals.get(a.rel).map_or(&[], |l| l.as_slice()),
            )
        {
            return Some(content);
        }
        // 3. v0.122 (slice 1): a local / parameter → its inferred type, and
        //    `self` → its receiver/agent type. Both read the retained analysis
        //    tables and run before the lexical fallback, which knows only
        //    declarations by name.
        if let Some(text) = a.snapshots.get(a.rel) {
            let local = a
                .locals
                .get(a.rel)
                .and_then(|locals| crate::locals_nav::describe_local_at(locals, text, a.offset))
                .or_else(|| {
                    let entries = a.expr_types.get(a.rel)?;
                    crate::symbols::describe_self_at(text, a.offset, entries)
                });
            if let Some(content) = local {
                return Some(content);
            }
        }
    }

    // The remaining rungs are lexical, over the **live** buffer.
    let (text, offset) = input.doc?;
    let Some((name, span)) = identifier_at(text, offset) else {
        // v0.121 (ADR 0156): the mechanical coverage test requires every
        // lowercase-initial keyword to have *a* hover path. A bare keyword token
        // (`requires`, `suite`, …) never resolves as an identifier above, so it
        // falls here — its one-line `keywords` registry doc, the same text
        // completion shows. Richer per-declaration hover is `describe_symbol`'s
        // job, not this fallback's.
        return crate::symbols::describe_keyword_at(text, offset).map(str::to_string);
    };
    // 4. A top-level declaration in this file (fast path).
    if let Some(content) = crate::symbols::describe_symbol(text, &name) {
        return Some(content);
    }
    // 5. v0.137.0 (ADR 0161) + #611 (gap A): the `key`/`store` contextual
    //    keywords, the agent state fields they declare, and — since #611 — a
    //    *reference* to one from the agent's body. Single-file-local, so it
    //    resolves before any project-wide scan.
    if let Some(content) = crate::symbols::describe_agent_state_at(text, span.start) {
        return Some(content);
    }
    // 6. v0.140 (ADR 0163): a handler-position `@cache` annotation — not a symbol
    //    and no local, so it resolves here beside the agent state.
    if let Some(content) = crate::symbols::describe_handler_annotation_at(text, span.start) {
        return Some(content);
    }
    // 7. v0.123 (slice 2, DECISION B): a `Recv.member` name-receiver access — a
    //    capability op (`Clock.now`), a refined/opaque `of`/`unsafe`, or a type
    //    static — via the same path signature help uses, over the project and the
    //    embedded surface. Before the cross-file / first-party name scans.
    crate::symbols::qualified_callee_at(text, span)
        .and_then(|callee| crate::signature_help::resolve_label(&callee, text, input.src_root))
        .map(|sig| format!("```bynk\n{sig}\n```"))
        // 8. A project-wide scan (v1.1), then 9. the embedded first-party sources
        //    (slice 9) — so `uses`/`consumes` names resolve across file
        //    boundaries (§3.4) and stdlib/surface symbols surface too.
        .or_else(|| {
            input
                .src_root
                .and_then(|root| crate::symbols::describe_symbol_cross_file(root, input.uri, &name))
                .map(|(_other_uri, desc)| desc)
        })
        .or_else(|| crate::symbols::describe_firstparty_symbol(&name))
}

/// The identifier-ish token covering `offset` — its text and span.
///
/// Hole-aware (issue #473): interpolation holes are expanded, so a cursor inside
/// `"… \(name) …"` lands on the hole's identifier token rather than the opaque
/// `InterpStr` token. The token-kind filter is wider than `Ident` because the
/// literal kinds carry hover-worthy names too (`Result`, `Option`, `Effect`).
fn identifier_at(text: &str, offset: usize) -> Option<(String, Span)> {
    use bynk_syntax::lexer::TokenKind;
    let tokens = bynk_syntax::lexer::tokenize_expanding_holes(text).ok()?;
    tokens
        .iter()
        .find(|t| {
            t.span.start <= offset
                && offset < t.span.end
                && matches!(
                    t.kind,
                    TokenKind::Ident
                        | TokenKind::Int
                        | TokenKind::String
                        | TokenKind::Bool
                        | TokenKind::Float
                        | TokenKind::Result
                        | TokenKind::Option
                        | TokenKind::Effect
                )
        })
        .map(|t| (text[t.span.start..t.span.end].to_string(), t.span))
}
