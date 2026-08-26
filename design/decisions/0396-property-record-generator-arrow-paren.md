# 0396 — gen_descriptor_entry parenthesises a record-typed binding's generator body

- **Status:** Accepted (v0.289.5)

**Context.** `gen_descriptor_entry` (`bynk-emit/src/project/tests_emit.rs`)
builds each `for all` binding's generator-descriptor object, including a
`gen: (rng: any) => …` field whose body is spliced directly from
`BindingGen::gen_ts` — the real `TsExpr` `gen_ts_for_ty` builds for the
binding's type. For a record-typed binding, `gen_ts_for_ty`'s `Record` arm
produces `TsExpr::Object` for a non-empty record, or its own `"{  }"`
sentinel (`TsExpr::Ident`, its double-space-quirk comment) for a zero-field
one — and TypeScript parses a `{` immediately after `=>` as a **block**, not
an object literal, not an arrow expression body (#1397). For a multi-field
record this is a real `tsc` syntax error (`a:` reads as a statement label,
then the next field's `b:` breaks the block-statement grammar); for a
single- or zero-field record it is worse — a syntactically valid *empty or
labelled-statement* block, so the generator silently returns `undefined` at
runtime instead of throwing. `bynk-check::test_suites::prop_binding_generable`
already accepts any record whose fields are themselves generable (including
zero fields, vacuously), so this was reachable production behavior for any
`for all r: SomeRecordType { … }` binding, not a dead path — just never
exercised, since no existing fixture used a record-typed binding.

Originally filed against the pre-conversion `format!("{{ {} }}", …)` text
(#1396's own review), which produced byte-identical output; #1406 (Arc C,
slice E) later consolidated all three call sites
(`emit_test_property_function`/`emit_test_history_property_function`/
`emit_contract_attack_function`) onto the one `gen_descriptor_entry` helper,
narrowing the fix to a single choke point without yet applying it (kept that
slice byte-neutral, same discipline #1396 used).

**Decision.** In `gen_descriptor_entry`'s `gen` field, wrap `bg.gen_ts` in
`TsExpr::Paren` whenever it is `TsExpr::Object` (non-empty record) or a
`TsExpr::Ident` whose text starts with `{` (zero-field record) — a `match`
guard covering both producers `gen_ts_for_ty`'s `Record` arm actually emits,
so both the "syntax error" and the "silently wrong" failure modes close at
the same site. Every other type's `gen_ts` (int/string/bool/sum/refined)
already starts with something other than `{`, so the fallback arm passes
them through unchanged — zero diff for every existing fixture. The `Ident`
arm matches by leading `{` rather than the sentinel's exact `"{  }"`
double-space spelling (review of #1425): a valid identifier can never start
with `{`, so this stays a no-op for every other shape while surviving a
future normalisation of that formatting quirk (the same cleanup
#1321/#1327/#1390 already invite for its other two copies).

**Consequences.** Two new fixtures pin the fix, each hitting a distinct arm
of the guard: `1397_property_record_binding` (a two-field `Point` record —
the `TsExpr::Object` arm) and `1397_property_record_binding_zero_field` (`type
Empty = {}`, valid bynk — the `TsExpr::Ident` sentinel arm, added during
review since no fixture reached it). Both confirmed to reproduce a real
regression without the fix — the `Point` fixture as a genuine `tsc` `TS1005`
syntax error, the `Empty` one as a golden-file text mismatch in the
byte-diff harness (`positive_fixtures`), since the pre-fix `(rng: any) => {
}` is *syntactically valid* JavaScript (an empty block, silently returning
`undefined`) and so cannot be caught by `tsc` or by a runtime assertion that
never observes the drawn value — exactly the "silently wrong" failure mode
this issue names, demonstrated rather than assumed. Both fixtures' own
`bynkc test` runs also pass end to end with the fix applied.

Review of this PR (#1425) raised three points, addressed as follows:

1. The `Point` fixture's original predicate was `expect true`, so the
   record's own fields were generated but never observed — the coverage
   claim wasn't end-to-end. Strengthened to `expect p.x <= p.y || p.y <=
   p.x`, a real field-reading comparison. This surfaced a genuinely separate,
   pre-existing gap: `for all`-bound `Int` values draw as JS `bigint`
   (`__bynkRng.int`), which throws `TypeError: Cannot mix BigInt and other
   types` against ordinary bynk arithmetic mixing in a plain number literal
   (`p.x + 1`) — confirmed to reproduce identically for the simplest scalar
   `for all n: Int { n + 1 == n + 1 }`, so it is **not** specific to records
   and not introduced by this fix. `<=` between two same-shaped generated
   fields stays bigint-to-bigint throughout, so the strengthened predicate
   exercises real field access/comparison without hitting that unrelated
   gap. Filed separately as #1426 rather than folded into this fix, matching
   this issue's own "tracked separately" precedent.
2. The zero-field sentinel's exact-string match was fragile against the
   documented formatting-quirk cleanup its comment already invites —
   addressed above (shape-based `starts_with('{')` guard) and pinned by the
   new zero-field fixture.
3. (Not applied, noted for the record.) The reviewer suggested the printer's
   own `Arrow` rendering (`bynk-ts/src/printer.rs`) could defensively
   parenthesise any object-literal body at the class level, closing this
   failure mode for every future caller rather than only the current one.
   Left to a future slice if a second caller ever needs it: `gen_descriptor_
   entry` is still the only place in `bynk-emit` that builds an `Arrow` over
   an object-literal body, so a printer-level change today would be
   speculative generality with no second call site to justify it.

Closes #1397.
