---
level: patch
changelog: A `for all` property binding over a self-recursive `Sum`/`Record` type no longer generates a bare `undefined` in a real, typed constructor/field position once the runner's random-generation depth is exhausted — `gen_ts_for_ty`'s own fallback now hands `canon_ts_for_ty` the same recursion budget the checker already used to accept the binding as generable, not an under-provisioned `1`, and only for a type the checker actually validated, so an unvalidated branching variant can't turn the fallback into a multi-megabyte emit
---

## ADR: property-gen-depth-exhaustion-fallback-budget

title: `gen_ts_for_ty`'s depth-0 fallback re-uses `PROP_GEN_DEPTH`, not a bare `1`, so `canon_ts_for_ty` gets the same recursion budget `prop_binding_generable` used to accept the binding

summary: `canon_ts_for_ty` and `prop_binding_generable` share the exact same recursion shape (unconditional `depth == 0` check at entry, one unit spent per `Sum`/`Record` step into a field, `Base` never itself gating on depth) — handing the fallback the same constant the checker validated against guarantees it bottoms out within it too

**Context.** `gen_ts_for_ty`'s own depth-0 fallback called `canon_ts_for_ty(ty,
types, 1, tys)` — a bare `1`, unchanged since the function's own v0.114
introduction (`0205fc67`), never revisited. For a self-recursive `Sum` like
`type T = Base(n: Int) | Cons(tail: T)`, once `gen_ts_for_ty`'s own
random-generation depth (`test_suites::PROP_GEN_DEPTH`, 12) is exhausted
partway down a `Cons` chain, the fallback's own budget of `1` is one level
short of `Base`'s own terminal `n: Int` field: `canon_ts_for_ty(T, ..., depth:
1, ...)` picks `T`'s first variant (`Base`) correctly, but its own recursive
call for `n`'s type (`canon_ts_for_ty(Int, ..., depth: 0, ...)`) hits the
unconditional `depth == 0` early return before ever checking that `Int` is a
`Base` type needing no further recursion at all — landing a bare `undefined`
inside `Base`'s real, `number`-typed constructor argument
(`T.Base(undefined)`), a hard `tsc TS2345` error
(`Argument of type 'undefined' is not assignable to parameter of type
'number'`). Confirmed to reproduce directly against `main` via `bynkc test`
and confirmed absent with the fix.

`bynk-check::test_suites::prop_binding_generable` — the checker's own
generability gate, which is why `T` is accepted as a `for all` binding at
all — validates only a recursive sum's **first** variant, using the same
`PROP_GEN_DEPTH` constant, and shares `canon_ts_for_ty`'s exact recursion
shape: `if depth == 0 { return false/undefined }` at entry, one unit spent
per `Sum`/`Record` step down into a field's own type, and `Ty::Base(_) =>
true` (no depth gating) inside the match. Because the two functions consume
depth identically at every step, a type the checker accepted as generable
within `PROP_GEN_DEPTH` levels is *guaranteed* to bottom out in
`canon_ts_for_ty` within that same budget — the bug was purely that the
fallback call site handed it `1` instead of the real number.
`canon_ts_for_ty`'s other call sites (`binding_gen`'s own boundary
construction, `gen_ts_for_ty`'s own Sum/Record `unwrap_or_else` paths) already
pass `test_suites::PROP_GEN_DEPTH` directly — this fallback was the one
outlier.

**Decision.** Change the fallback to `canon_ts_for_ty(ty, types,
test_suites::PROP_GEN_DEPTH, tys)`, matching every other call site. No
change to `canon_ts_for_ty`'s or `prop_binding_generable`'s own internal
recursion shape — the parity argument above shows that's unnecessary; the
budget alone was wrong.

Review of this PR (#1434) raised one finding, addressed: the parity argument
above only holds for the type `prop_binding_generable` actually validated —
but `prop_binding_generable` only ever descends a `Sum`'s **first** variant,
while `gen_ts_for_ty`'s own `Sum` branch builds a thunk for **every**
variant. So `ty` at the depth-0 fallback can be a type reached only through
a non-first variant, never validated at all — e.g. `Nope(b: Bad)` on
`T = Plain(n: Int) | Nope(b: Bad)`, where `Bad = Node(l: Bad, r: Bad) |
Leaf(n: Int)` is itself first-variant-unterminating (`Node` never bottoms
out on its own). Handing such a type the full `PROP_GEN_DEPTH` budget
regardless means `canon_ts_for_ty`'s own "always expand the first variant"
strategy walks a full branching tree before ever reaching `Leaf`'s escape —
confirmed to reproduce a real ~176MB, 50+-second emit for exactly this
shape (a plausible emit-time hang/OOM, not merely an invalid file) once
this fallback started handing out 12 levels of budget instead of 1. Fixed
by gating the raised budget on `test_suites::prop_binding_generable(ty,
types, PROP_GEN_DEPTH, tys)` actually holding for `ty` itself — cheap to
check (the same "first variant only, short-circuits on the first failing
field" shape as `canon_ts_for_ty`, so at most `PROP_GEN_DEPTH` recursive
calls, not a tree walk) — falling back to the original, deliberately
conservative `1` otherwise. Confirmed empirically both ways: the guarded
fallback produces byte-identical output to the pre-#1429 baseline for the
pathological `Bad` case (208,029 bytes, ~0.25s, vs. 176MB/50s+ unguarded),
while still fixing the original `T` case.

**Consequences.** One new positive fixture,
`1429_property_recursive_sum_depth_exhaustion`, pins the original fix with
the issue's own literal repro (golden-text + `tsc_verify`, like #1427's own
fix — this bug is a hard `tsc --strict TS2345` error, so no new behavioural
test file is needed for it). The exponential-blowup regression itself
cannot be pinned as a positive fixture — the pathological `Bad`/`T` shape is
still genuinely invalid TypeScript with or without the guard (`Bad` was
never validated as generable in the first place; that's a separate,
pre-existing gap, not this fix's scope), so a `.bynk` fixture exercising it
would correctly fail `tsc --strict` and break `positive_fixtures`'
no-exclusions gate. Pinned instead by two direct unit tests on
`gen_ts_for_ty` (`bynk-emit/src/project/tests_emit.rs`) — one confirming the
checker-validated case still bottoms out without `undefined`, one asserting
the unvalidated branching case's own output stays under a generous bound
(2,000 chars, against a confirmed ~86,000-character unguarded failure) —
each confirmed to fail against the pre-guard version of the function before
being confirmed to pass against the guarded one. No existing fixture
changes; `cargo xtask greenfield-status --apply` shows zero gated-probe
diff (only the `test_density` trend probe moves, from the two new unit
tests).

Closes #1429.
