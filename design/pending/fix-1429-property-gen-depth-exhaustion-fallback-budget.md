---
level: patch
changelog: A `for all` property binding over a self-recursive `Sum`/`Record` type no longer generates a bare `undefined` in a real, typed constructor/field position once the runner's random-generation depth is exhausted — `gen_ts_for_ty`'s own fallback now hands `canon_ts_for_ty` the same recursion budget the checker already used to accept the binding as generable, not an under-provisioned `1`
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

**Consequences.** One new fixture,
`1429_property_recursive_sum_depth_exhaustion`, pins the fix with the
issue's own literal repro. No new behavioural test file needed: like #1427's
own fix, this bug is a hard `tsc --strict` `TS2345` error today, so
`tsc_verify`'s own workspace-wide gate catches a regression on its own. No
existing fixture changes; `cargo xtask greenfield-status --apply` shows zero
diff (this fix touches no `write!`/`format!`/`any` surface).

Closes #1429.
