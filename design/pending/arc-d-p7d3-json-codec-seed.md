---
level: patch
changelog: The JSON codec seed collector (R8.14) reads a resolved `Callee::Intrinsic`, not a bare receiver-name match — closing Arc D, the final settling slice
---

## ADR: p7d3-json-codec-seed-callee
title: The JSON codec seed reads Callee::Intrinsic, revisiting P6.56's declined attempt for real
summary: P6.56's own stated blocker no longer holds, closing R8.14's evidenced gap and a narrow shadow-safety correctness gap

**Context.** `collect_json_codec_roots` (`bynk-emit/src/emitter.rs`) matched `Json.encode`/`decode`
call sites by a bare `id.name == JSON` receiver-name check, with no semantic verification. Its own
doc comment recorded why, citing P6.56: "`resolver.rs` resolves `Json.encode`/`decode` as a
no-declaration-needed built-in static... `commons.callees` carries no entry for this call site at
all. There is no IR-native value to read here." That citation no longer holds — `checker::calls`'s
own JSON-static dispatch now inserts `Callee::Intrinsic { ns: JSON, op }` for exactly this call
shape, guarded against a local shadow the same way the numeric-parse/`Duration`/`Bytes`/`Stream`
statics immediately alongside it already are.

**Decision.** Route `collect_json_codec_roots` through `commons.callees.get(&e.id)`, reading
`Callee::Intrinsic { ns: JSON, op }` instead of the bare receiver-name match. Checked the boundary
seed's own current collector (consumed-cross-context-service roots) for a comparable gap and found
none — it already reads typed `CrossContextService` fields and a resolved `TyId`, not a raw AST walk
in the problematic sense the json seed was, so no change there. The wider "unified into one collector
over `bynk-ts` tree nodes" framing this slice was originally scoped under is not what actually closes
R8.14: `serialisation::collect_codec_closure` (the shared transitive-closure function every root list
already feeds through) was already the "one collector, parameterised" half the rule asks for; no
tree-node reorganisation was needed or built.

**Consequences.** Closes R8.14's own "AST-shaped, not checker-resolved" framing and a real, narrow
correctness gap: a local variable or type named `Json` shadowing the builtin would previously have
been misread as a genuine codec call — `Callee::Intrinsic`'s presence is the checker's own
already-verified "this really is the builtin" answer. No fixture in the current corpus shadows
`Json`, so no output changes; full workspace test suite confirms zero diff. This is Arc D's final
remaining slice — landing it closes the whole settling track (8 of 8).
