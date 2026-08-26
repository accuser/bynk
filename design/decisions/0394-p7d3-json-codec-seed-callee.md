# 0394 — The JSON codec seed reads Callee::Intrinsic, revisiting P6.56's declined attempt for real

- **Status:** Accepted (v0.289.3)

**Context.** `collect_json_codec_roots` (`bynk-emit/src/emitter.rs`) matched `Json.encode`/`decode`
call sites by a bare `id.name == JSON` receiver-name check, with no semantic verification. Its own
doc comment recorded why, citing P6.56: "`resolver.rs` resolves `Json.encode`/`decode` as a
no-declaration-needed built-in static... `commons.callees` carries no entry for this call site at
all. There is no IR-native value to read here." That citation no longer holds — `checker::calls`'s
own JSON-static dispatch now inserts `Callee::Intrinsic { ns: JSON, op }` for exactly this call
shape, guarded against a local shadow the same way the numeric-parse/`Duration`/`Bytes`/`Stream`
statics immediately alongside it already are.

**Decision.** Route `collect_json_codec_roots` through `commons.callees.get(&e.id)`, reading
`Callee::Intrinsic { ns: JSON, op }` instead of the bare receiver-name match. Review found this
first cut one-sided: `lower_json_codec_call` (the emission half deciding whether a call site
*references* a codec helper) still matched the same bare receiver name, so a shadowed `Json` would
collect no seed (no helper emitted) while the still-syntactic lowering emitted a reference to one
anyway — worse than the pre-existing behaviour, not better. A new fixture
(`1422_json_codec_shadowed_by_local_binding`, arity-matched to real `Json.encode` so an unrelated
arg-count guard couldn't mask the bug) empirically confirmed this: reverting only the lowering half
produces `JSON.stringify(note as JsonValue)` for a call that should read `Box.encode(Json, note)` —
a real silent miscompilation. Fixed by routing `lower_json_codec_call` through the identical
`Callee::Intrinsic` read. Checked the boundary seed's own current collector (consumed-cross-
context-service roots) for a comparable gap and found none — it already reads typed
`CrossContextService` fields and a resolved `TyId`, not a raw AST walk in the problematic sense the
json seed was, so no change there. The wider "unified into one collector over `bynk-ts` tree nodes"
framing this slice was originally scoped under is not what actually closes R8.14:
`serialisation::collect_codec_closure` (the shared transitive-closure function every root list
already feeds through) was already the "one collector, parameterised" half the rule asks for; no
tree-node reorganisation was needed or built.

**Consequences.** Closes R8.14's own "AST-shaped, not checker-resolved" framing and a real
correctness gap, on both the seed and the emission side: a local variable or type named `Json`
shadowing the builtin is now resolved as the ordinary call it is, on both halves — `Callee::
Intrinsic`'s presence is the checker's own already-verified "this really is the builtin" answer.
The new fixture pins this permanently; it is the first direct test either half of this collector has
had. Full workspace test suite confirms zero diff elsewhere. This is Arc D's final remaining slice —
landing it closes the whole settling track (8 of 8).
