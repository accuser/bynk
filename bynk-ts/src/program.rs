//! [`TsProgram`]/[`TsStmt`] — the tree, so far only wide enough for the
//! `Verbatim` escape hatch (Q2, `design/tracks/the-typescript-tree.md` §3.2).
//! P7.8 (gated on this crate existing) adds the real `TsStmt` variants the
//! reference's own §7.1 sketch names; deliberately not pre-built here
//! (`bynk-ts`'s own module doc, and #1307's Decision E) — a generic `TsNode`
//! wrapper speculating about `TsExpr`/`TsType`/`TsDecl`'s own shape before
//! any of the three exist would be guessing, not designing.

use bynk_syntax::span::Span;

/// A whole generated TypeScript module, as an ordered sequence of top-level
/// statements. `Vec<TsStmt>`, plain — no richer container yet (P7.6's own
/// `Artefacts { docs: BTreeMap<PathBuf, Document> }` is where a *project's*
/// documents get keyed; this is one document's own tree).
#[derive(Debug, Default)]
pub struct TsProgram {
    pub stmts: Vec<TsStmt>,
}

impl TsProgram {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, stmt: TsStmt) {
        self.stmts.push(stmt);
    }
}

/// One top-level statement. Right now, always a [`VerbatimOrigin`]-tagged
/// escape hatch — constructible only via [`TsStmt::verbatim`], not as a bare
/// struct literal (`#1307`'s Decision D): one named constructor gives the
/// `verbatim_sites` probe (`xtask/src/greenfield_status.rs`) exactly one
/// string to line-scan for, the same discipline `bynk-emit::emitter::
/// toml_doc`'s `TomlEntry::kv`/`TomlBlock::table` already established (P7.3)
/// — not a new pattern here, an extension of one already in this codebase.
#[derive(Debug)]
pub struct TsStmt {
    pub(crate) kind: TsStmtKind,
    /// Where this statement's content originated in the `.bynk` source, if
    /// known — the printer records a source-map checkpoint from this field
    /// directly (R7.4: the source map comes from the printer reading
    /// `TsNode.span`, not a phase before it). `None` for content with no
    /// real originating span (rare; most `Verbatim` construction sites have
    /// one).
    pub span: Option<Span>,
}

#[derive(Debug)]
pub(crate) enum TsStmtKind {
    Verbatim {
        #[allow(dead_code)]
        // read by the lint's own violation attribution once Arc C gives it real content to report on; not yet, per Decision F
        origin: VerbatimOrigin,
        text: String,
    },
}

impl TsStmt {
    /// The one constructor for a `Verbatim`-kinded statement.
    pub fn verbatim(origin: VerbatimOrigin, text: impl Into<String>, span: Option<Span>) -> Self {
        Self {
            kind: TsStmtKind::Verbatim {
                origin,
                text: text.into(),
            },
            span,
        }
    }

    /// This statement's own wrapped text, whatever its kind — today always
    /// `Verbatim`'s. `pub(crate)`: only the printer and the lint need this;
    /// nothing outside the crate reads a statement's content directly
    /// (R7.6 — downstream consumers couple to nodes, never to emitted text).
    pub(crate) fn text(&self) -> &str {
        match &self.kind {
            TsStmtKind::Verbatim { text, .. } => text,
        }
    }
}

/// Which family of residual, not-yet-converted emission a [`TsStmt::verbatim`]
/// statement came from. A closed enum, deliberately — "makes the ratchet a
/// compile-time construct, not a grep" (Q2's own settling text). Named
/// file-by-file as Arc C actually needs them (`ast_importers`'s own five-file
/// floor is the precedent for how this track names residue), not
/// pre-populated for the whole ~19-slice Arc C schedule up front — only the
/// three files Arc C's own first slice (`design/tracks/the-typescript-tree.md`
/// §6) already commits to converting next.
///
/// Deliberately **not** `#[non_exhaustive]`: a `match` over every variant —
/// in this crate, or in `bynk-emit` once Arc C reads it — must fail to
/// compile the moment a new variant is added, forcing every consumer to
/// account for it explicitly. A non-exhaustive enum would let a wildcard arm
/// silently absorb a new residue family instead, exactly the "grep, not a
/// compile-time construct" Q2's own settling text rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerbatimOrigin {
    /// `bynk-emit/src/emitter/contracts.rs`.
    Contracts,
    /// `bynk-emit/src/emitter/secrets.rs`.
    Secrets,
    /// `bynk-emit/src/emitter/runtime_use.rs`.
    RuntimeUse,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_prints_its_statements_in_push_order() {
        let mut program = TsProgram::new();
        program.push(TsStmt::verbatim(
            VerbatimOrigin::Contracts,
            "const a = 1;",
            None,
        ));
        program.push(TsStmt::verbatim(
            VerbatimOrigin::Secrets,
            "const b = 2;",
            None,
        ));
        let texts: Vec<&str> = program.stmts.iter().map(TsStmt::text).collect();
        assert_eq!(texts, vec!["const a = 1;", "const b = 2;"]);
    }

    #[test]
    fn verbatim_carries_its_own_span() {
        let span = Span::new(3, 8);
        let stmt = TsStmt::verbatim(VerbatimOrigin::RuntimeUse, "x", Some(span));
        assert_eq!(stmt.span, Some(span));
    }
}
