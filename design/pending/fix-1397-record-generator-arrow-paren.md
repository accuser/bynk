---
level: patch
changelog: A record-typed `for all` property binding's generator arrow now wraps its object-literal body in parens, closing an unparseable/silently-wrong generated-TypeScript gap
---

## ADR: property-record-generator-arrow-paren
title: gen_descriptor_entry parenthesises a record-typed binding's generator body
summary: An object-literal arrow body needs parens or TypeScript reads it as a block

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
`TsExpr::Paren` whenever it is `TsExpr::Object` (non-empty record) or the
`"{  }"` `TsExpr::Ident` sentinel (zero-field record) — a `match` guard
covering both producers `gen_ts_for_ty`'s `Record` arm actually emits, so
both the "syntax error" and the "silently wrong" failure modes close at the
same site. Every other type's `gen_ts` (int/string/bool/sum/refined) already
starts with something other than `{`, so the fallback arm passes them
through unchanged — zero diff for every existing fixture.

**Consequences.** New fixture `1397_property_record_binding` (a two-field
`Point` record bound in a `for all`) pins the fix: confirmed to reproduce a
real `tsc` `TS1005` syntax error without it (`bynkc test` against the
fixture with the fix reverted), and to compile, type-check, and run
successfully with it (`bynkc test` — the property passes). Closes #1397.
