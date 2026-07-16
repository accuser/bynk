# 0199 — One codec path at the workers boundary: the cross-context edge is generated, not asserted

- **Status:** Accepted (v0.176)
- **Provenance:** issue #642, the first half of the design-review finding #550
  ("Type the workers boundary + cross-context contract-hash check", §5.1(2), §8
  Platform #3). Closes the "Workers-edge type safety" shortfall admitted in
  `bynk-status-and-roadmap.md` §3 and its hygiene item 2. The finding's second
  half — the deploy-skew contract hash — is #643 and is **not** in this
  increment. **Closes #642.**
- **Spec:** `emission.md` (§7.3.4b, the cross-context boundary codec).
- **Realises:** every value crossing a `workers` cross-context boundary is
  encoded and decoded by a generated codec — the same monomorphised helpers the
  `bundle` target and the `Json` codec already use. No wire position asserts a
  value through `as JsonValue`; no return type decodes through an unvalidated
  identity function.
- **Discharges:** [0142](0142-bytes-primitive.md) D8's deferral to "the roadmap's
  typed cross-context boundary fix". `bynk.types.bytes_at_workers_boundary` is
  withdrawn.
- **Relates:** [0092](0092-cross-context-caller-value.md) (the seam this types —
  its `X-Bynk-Caller` header and wire format are untouched),
  [0197](0197-generic-record-boundary-codecs.md) and
  [0183](0183-generic-record-types.md) (the monomorphised `GenericInst` machinery
  this routes the boundary onto), [0124](0124-agent-state-rehydration.md) (the
  rehydration gate, which shares the codec path and inherits an improvement here).

## Context

Bynk's pitch is independently deployable contexts, so the cross-context call is
*the* seam the language exists to make safe. It was the one place the compiler
gave up and asserted. The status doc admitted it plainly: "the `bundle` path is
fully typed; `workers`-mode boundary emission leans on `any` plus runtime
serialisation helpers, so static guarantees are weakest exactly at the edge."

The cause was **three parallel codec dispatches**, not one:

1. `serialisation.rs` — the real machinery. `GenericInst` monomorphises a codec
   per instantiation (`serialise_List_User`, and since ADR 0197
   `serialise_Paginated_User`). Correct and complete.
2. `workers_entry.rs`'s `serialise_call` / `deserialise_call` — the callee side.
3. `emit.rs`'s `workers_serialise_expr` / `workers_deserialise_ref` — the caller
   side, and the weakest.

Two and three shadowed one, and drifted from it. The drift was not uniform, which
is what made it dangerous:

- **Asymmetric within a dispatch.** `deserialise_call` base64-*decoded* a `Bytes`
  while `serialise_call` cast it to `JsonValue`. A `Bytes` therefore
  mis-round-tripped — which is why ADR 0142 D8 had to *diagnose* a bare `Bytes` in
  a `workers` signature rather than emit it. The restriction was never about
  `Bytes`; it was about the boundary having two dispatches that disagreed.
- **Asymmetric across dispatches.** `workers_deserialise_ref` handled `List`/`Map`;
  `workers_serialise_expr` dropped them to a catch-all cast. A `List[T]` argument
  was asserted outbound and validated inbound.
- **An unvalidated escape.** An unresolved return type lowered to
  `((j: any) => ({ tag: "Ok", value: j }))` — an `Ok` over a bare `any`, with no
  validation whatsoever.

None of this was visible from source: the same `.bynk` program was fully typed
under `--target bundle` and quietly `any` under `--target workers`. An author
could not hand-roll the fix — the codec is generated, the wire format is the
compiler's.

## Decisions

**[DECISION A] Collapse the three dispatches into one; delete the fallbacks
rather than improve them (Recommended: yes).** The caller and callee sides become
one line each over `serialisation.rs`'s dispatch, threaded with a namespace prefix
(`""` module-local, `"handlers."` from a Worker entry). The cast and
identity-function arms are deleted, not patched: an `as JsonValue` is exactly the
assertion this increment exists to remove, and keeping one arm keeps the class of
bug. `List`/`Map` become symmetric, `Bytes` base64-encodes because its helper
already did, and the edge's guarantees match the bundle path's.

**[DECISION B] The unified dispatch must not lose the precision either
predecessor had — which means splitting the two failure modes, not picking one
predecessor's answer (Recommended: split).**

Unifying surfaced that the two predecessors were imprecise in *opposite*
directions, each in the field the other got right, because both collapsed two
distinct failures into one error:

- **`expected`:** `serialisation.rs` reported the `typeof` it tested, so an `Int`
  given `3.5` read `expected: "number", actual: "number"` — useless in exactly the
  case the integrality check exists to catch. The workers path reported the
  requirement (`"integer"`).
- **`actual`:** the workers path reported `String(value)` — the offending *value*
  (`"NaN"`, `"3.5"`). `serialisation.rs` reported the `typeof`.

The resolution is not to take one of each. `String(value)` is not safe to adopt
wholesale: on a *`typeof`* failure the value can be anything, so an `Int` sent
`"hunter2"` reported `actual: "hunter2"` — echoing an arbitrary caller-supplied
value into a 400 response body, against ADR 0107's discipline of never reporting
the offending value. And `typeof` is not adequate either, per the `expected` row.

So the arm **splits the failure modes**, which dissolves the trade:

- a wrong `typeof` reports the `typeof` — the value could be anything, so it is
  never echoed;
- a *failed predicate* means the `typeof` already matched, so the value is
  provably a **number**: `String(__v)` is `"3.5"` for a non-integer `Int`, and
  provably one of `"NaN"` / `"Infinity"` / `"-Infinity"` for a non-finite `Float`
  — a closed set.

The result is strictly more precise than either predecessor in both fields, with
strictly less exposure than the workers path. This is the increment's clearest
argument for one path: the fix lands once, so the **pre-existing** `Json` and
agent-rehydration paths (ADR 0124) — which had always carried the vaguer message
*and* the safe-but-blunt `actual` — improve for free.

_(An earlier revision of this decision kept `actual: typeof` and claimed nothing
was lost. That was wrong: the workers path's `actual` precision was silently
dropped, reproducing one field over the same uselessness the `expected` fix
removed. Caught in review of #648.)_

**[DECISION C] An unresolvable consumed signature is an internal assertion, not a
user diagnostic (Recommended: assert).** The checker resolves the call before the
emitter runs, so the emitter failing to find the signature means the emitter
disagrees with the checker — a compiler bug. Shipping an unvalidated `any` to
production to paper over a compiler bug is the worst trade available. #642
proposed a `bynk.emit.unresolved_cross_context_signature` **diagnostic**; that
framing was wrong and is corrected here. A diagnostic is a statement about the
*author's* program, and no author action can cause or fix this. It is a `panic!`
carrying the invariant, in the same register as the emitter's existing
`unreachable!` for the confined type family.

**[DECISION D] Retire `bynk.types.bytes_at_workers_boundary`; remove the code from
the registry (Recommended: retire).** The guard was a consequence of Decision A's
cast path. With one symmetric dispatch it has no cause, so ADR 0142 D8's deferral
is discharged. Retiring a *rejection* only widens what compiles, so no program
breaks. #642 proposed retaining the id "as retired, per the established practice";
that was wrong — the repo's actual practice, enforced by the
`registry_matches_codes_used_in_source` drift guard, is that a code no longer
raised in source is **removed** from the registry. The historical record lives in
ADR 0142, ADR 0143, and the changelog, which are left as they stand: they record
what was true then.

**[DECISION E] Type the `on call` compose-root parameters; leave the other
wrappers `any` (Recommended: scoped to `on call`).** A compose wrapper's
parameters carried `: any`, which meant a wrong codec still type-checked — so the
only guard against regression would have been the goldens. They now carry the real
TS type, qualified against the `handlers` namespace the file already imports.

Scoped to `on call` deliberately. The other wrappers' parameters are not all
codec-produced: an HTTP or WebSocket wrapper mixes a deserialised `body` with
route/query params the entry lifts from the URL as raw strings, so typing them
against the *declared* type would assert a coercion that seam does not perform.
Those are separate boundaries with their own extraction rules.

This decision paid for itself immediately. With `any` removed, `tsc --strict`
found two latent bugs the erosion had been hiding:

- **A brand gap.** A context rebrands the commons types it `uses` (`Money &
  { __ctxBrand: "commerce.orders" }`), but the boundary codec lives in the commons
  module and returns the *unbranded* type, while the handler it feeds is typed
  against the branded one. The entry now re-asserts through `unknown` — the
  established spelling, and the same reasoning the bundle path's cross-context
  argument cast already used.

  **The assertion is scoped to imported types, and the narrowness is the point.**
  An `as unknown as T` is exactly the unchecked assertion this increment exists to
  delete, so applying it one position wider than the gap it bridges would take
  back what Decision E just bought: for a **context-declared** type there is no
  brand gap at all — `handlers.ts` declares and exports it, and `deserialise_<T>`
  already returns precisely `handlers.<T>` — so asserting there would let a wrong
  codec type-check again on the entry→compose link, moving the check rather than
  landing it. The predicate is absence from the unit's own `table.types`, which
  holds only local declarations, so absence means the name was imported. (The
  first revision keyed on *any* named root and was this much too broad; caught in
  review of #648.)
- **A missed import.** The `Bytes` runtime-helper injection anchored on the
  `type ValidationError` binding its target line happened to carry. Once a `Bytes`
  could reach a Worker *entry* (Decision D), that file referenced
  `__bynkBytesFromBase64` and named no `ValidationError` — so the injection
  silently did not fire. The anchor is now the runtime import itself.

Both were pre-existing and neither is reachable from the goldens alone.

**[DECISION F] The runtime-owned error types keep their pass-through, and it is
named (Recommended: name it, do not fix it here).** `ValidationError`, `JsonError`,
`HttpResult` and `QueueResult` are declared by the runtime, not by a `TypeDecl` the
emitter can walk, so there is no helper to generate and no codec to name. They
keep the pass-through the whole boundary used before this increment. Their JSON
shape is fixed by the runtime, so the cast is unchecked rather than wrong. It is
the one remaining unchecked arm at the boundary, stated in `emission.md` §7.3.4b
rather than left to be discovered.

**[DECISION G] A context still borrows its callee's codecs; self-contained
Workers are deferred (Recommended: defer, and say why).** #642's Decision B —
each context generating its *own* codecs from `consumed_types`, dropping the
cross-Worker source import — is **not** in this increment. Grounding it showed it
is a distinct emitter increment, not a detail of unifying the dispatch: a
caller-side codec for a callee-owned type must *name* that type in its return
position, and `emit_refined` reaches for the type's own `.of` constructor — so
the caller needs its own view of the callee's type declarations, which touches the
branding model.

This is worth stating precisely because it bounds what this increment claims.
Today `commerce.orders` reaches `commerce_payment.deserialise_Result_AuthId_PaymentError`
through a runtime import of the callee's module, so a `workers` build has exactly
**one** compiled view of each contract, borrowed. The boundary is now *typed*; it
is not yet *verified*. That deferral is the prerequisite for #643's contract hash,
which compares the caller's view against the callee's — and which would verify
nothing while there is only one view.

## Consequences

- Three dispatches become one (net −67 lines in the emitter). `emit.rs`'s
  `workers_serialise_expr` / `workers_deserialise_ref` / `workers_inner_ts_name`
  are deleted; `workers_entry.rs`'s `serialise_call` / `deserialise_call` become
  delegating one-liners. `serialisation.rs` gains the `_via` (namespace-threaded)
  forms plus the `Unit` and runtime-error arms the `Json` codec path never needed,
  because the checker's codec-domain rule rejects them there while the
  cross-context boundary admits them (`on call () -> Effect[Result[(), E]]`).
- A bare `Bytes` crosses a `workers` cross-context boundary (fixture 376, which
  replaces the retired negative fixture 270 — the diagnostic's own fixture,
  inverted). The `Json`/rehydration paths inherit Decision B's precision fix.
- Every one of the 205 project fixtures passes `tsc --strict --noEmit`, now with
  typed `on call` compose roots — which is the property that makes the win
  checkable rather than asserted.
- **Not closed:** #550. This increment is its first half. The deploy-skew hole it
  names is untouched: nothing at runtime verifies that a deployed callee matches
  what its caller was compiled against, and `deploy --context NAME` still
  institutionalises the skew. #643 carries that, on top of Decision G.
