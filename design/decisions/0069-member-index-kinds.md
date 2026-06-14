# 0069 — Member index kinds via compound names

- **Status:** Accepted (v0.36)
- **Spec:** `design/karn-lsp-spec.md` §3 (index coverage)
- **Relates to:** ADR 0053 (the binding index), ADR 0064 (locals, the other deferred kind), ADR 0067 (call hierarchy, whose method edges this lights up)

## Context
The binding index records only top-level kinds. Instance methods, record fields,
and capability ops — all parent-scoped (`T.m`, `T.field`, `Cap.op`) — were deferred
across v0.25/v0.27/v0.28/v0.34, so go-to-def, references, rename, and semantic
tokens went dark on them and call hierarchy skipped method calls. The enabling
fact: the parent's type is **already resolved at the use site in the checker**
(`check_method_call` has the receiver type, field access has the record type, op
dispatch has the capability), so a use can be recorded already spelled
`"Parent.member"`.

## Decision
Index members as first-class symbols using **compound names** in the existing flat
`SymbolKey { unit, kind, name }` — `name = "Parent.member"`, plus a new
`SymbolKind` variant per member kind. **Slice 1** landed methods
(`SymbolKind::Method`); **slice 2** lands record fields (`SymbolKind::Field`,
`"Type.field"`) and capability ops (`SymbolKind::CapabilityOp`, `"Cap.op"`) with
the same mechanism.

- **No new key shape.** The compound name rides the existing qualification: the
  def is `add_def`'d under the parent's unit with the compound name; the ref is
  recorded **bare** at the checker's resolved use site (`ctx.refs.record(span,
  Method, "T.m")`) and resolved by the same `uses`/`consumes` walk as a cross-file
  type reference. No assembly-time parent injection (the scouted risk, avoided).
- **Spans are member-segment only.** The def site is the method-name span; the ref
  site is the call's method span. Both cover just `m` (never `T.`), so rename edits
  the segment alone.
- **Rename is made compound-aware.** The edit replaces the member segment, so:
  `remap_site`'s length delta is against the **segment** length (not the compound
  key length), and the post-rename key is the **prefix plus new segment**
  (`renamed_key_name`), not the bare new name. The same-name no-op check compares
  segments. The two rename validators (re-analyse + index-equality-modulo-rename)
  otherwise apply unchanged.
- **Call-hierarchy method edges light up.** `add_def` populates `owner_keys` for
  the `"T.m"` owner, so methods are valid callers; the `CallEdge` build accepts
  `Method` callees alongside `Fn`. Capability-op call-graph edges stay out of
  scope — call hierarchy is the fn/method relation; op dispatch is a different
  (effectful capability-use) relation.
- **Fields are recorded from every reference form** (slice 2) — read access,
  construction labels, and spread overrides — each resolved against the record
  type in scope at the checker site, so rename touches all occurrences (not just
  reads). Capability ops are recorded from both the local and cross-context call
  paths (the latter `record_in_unit` into the providing unit, where the op def
  lives). Fields colour as the standard `property` token; ops reuse `method`.
- **Semantic tokens.** `SymbolKind::Method` maps to the standard LSP `method`
  token type, **appended** to the legend (index 7) so existing indices are
  unchanged; `method` is a built-in VS Code type, so no extension declaration is
  needed (it joins `STANDARD_TYPES` in the drift guard).

## Consequences
Methods become navigable, renamable, and colourable, and call hierarchy records
method calls — all from a ref recorded at the checker's resolved use site, with no
new key shape and no analysis export. The compound-name convention generalises to
fields and ops (slice 2). Locals (rename only, a separate side-channel) and generic
type parameters (coarse `TypeRef` spans) stay deferred.
