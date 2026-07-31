# 0308 — The wire-contract IR derives from the AST + type table, not `checker::Ty`

- **Status:** Accepted (v0.246)

**Context.** `bynk-check`'s checker produces `Ty::Named { name, kind, args }`
for a named type — the representation everything downstream of type-checking
already uses. Deriving the wire-contract IR from `Ty` instead of from the raw
`TypeDecl` + type table was the more obvious-looking option, since it is
already the checker's own settled vocabulary for "what type is this."

**Decision.** Derive from the AST (`TypeRef`/`TypeDecl`) plus the type table
instead, mirroring exactly how `contract.rs` already derives its own
canonical form. Two independent reasons converge on the same answer:

1. **`Ty` is lossy for what this IR needs.** `Ty::Named` carries a type's
   name, its `kind`, and its generic `args` — never its refinement
   predicates. Deriving `WireScalar::predicates`/`revalidation` from `Ty`
   would still require the identical type-table lookup this module already
   does directly; `Ty` would buy only generic-argument substitution, which
   the moved-verbatim walks (`collect_type_names`, `subst_type_ref`, …)
   already implement over `TypeRef` without it.
2. **`contract.rs` sets the precedent this module must not break.**
   `contract.rs` lives in `bynk-check` and derives its own canonical form
   from the AST + type table directly — it cannot depend on the checker
   having run, since a checker pass over one file cannot see another
   context's declarations at all in single-file mode. `wire.rs` keeps the
   same shape of dependency for the same reason, and for a reason `contract.rs`
   does not share: the peek (`bynk_ide::wire_contract`'s `wire_contract_at`)
   must answer for a file with **errors** — the whole point of a hover peek
   is that it works while the author is mid-edit, and a `checker::Ty` derived
   only where the checker ran clean would go blank exactly when the author
   most wants to see the boundary shape.

**Consequences.** The IR construction functions
(`wire_ref`/`wire_type`/`boundary_model`) take `&HashMap<String, Arc<TypeDecl>>`,
never a `Ty` or an `expr_types` table. Where the peek genuinely does need a
checker fact it cannot get any other way — disambiguating a bare `Ok(_)`
literal from an ordinary same-shaped value for the HTTP response walk — it
reads `expr_types` directly and **degrades to the declared-return-type
heuristic** when that table is empty (ADR 0063's clean-file ceiling), rather
than pulling the whole IR through a checker dependency it does not otherwise
need. That fallback is documented at its one call site
(`bynk_ide::wire_contract::ResponseWalk::is_http_result_expr`) and in
`design/bynk-lsp-spec.md` §3.26, rather than left to look exact when it is
not.
