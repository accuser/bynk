---
level: minor
changelog: "`is` bindings now flow where their test is known to have matched (#1654, type-system §2.3.6): into the right operand of `&&` and `implies` (`o is Some(v) && v > 0`), and into the `else` of a negated test (`if !(o is Some(v)) { 0 } else { v }`). Both shapes were rejected with `unknown name`. `if n is Q && m is Q { … m … }` (refined `Q`) was accepted and then failed `tsc` (`__r1` used out of scope); it now compiles. One rule decides where a binding is in scope, and the resolver, the checker and the emitter all read it. Separately, a `--target workers` build is now `tsc`-clean in two cases that failed before (#1655): `compose.ts` imports the runtime types a service parameter names (`Option`, `Result`, …), and the build no longer writes unit test modules that imported the bundle layout. Integration suites, which target the workers layout, are still written."
---

## ADR: is-binding-scope
title: An `is` binding is in scope wherever its test is known to have matched
summary: One structural rule (which `is` tests an expression proves, by outcome) decides is-binding scope for the resolver, checker and emitter alike

**Context.** `is` bindings flowed only into an `if`'s then-branch, through
`&&` and parentheses. Three walks decided this independently:
- the resolver's `collect_is_binding_names`;
- the checker's `collect_is_bindings`;
- the emitter's `gather_is_bindings_for_emit`.

They had drifted:
- the checker scoped bindings into the right operand of `&&`/`implies`, but the
  resolver didn't, so `o is Some(v) && v > 0` failed with `unknown name 'v'`;
- nothing scoped an else-branch, so the negated form that type-system §2.3.6
  requires was rejected;
- the emitter lowered the right operand of `&&` inside an IIFE (keeping it
  lazy) and declared its receiver temp there, so a then-branch binding that
  read the temp failed `tsc` (TS2304).

**Decision.**
1. **One rule** (`bynk_check::narrowing::matched_is_tests`): given an expression
   and whether it evaluated true or false, the `is` tests that must have
   matched.
   - `e is P` proves itself when true.
   - `a && b` proves both operands when true.
   - `a || b` proves both operands when false.
   - `!e` swaps the outcome.
   - `a implies b` proves `a` when true and `b` when false, both when false.
   - Parentheses are transparent.

   The resolver, the checker and the emitter all map these tests to names,
   types or lowered declarations. A shape that one of them proves but another
   doesn't can no longer arise (Decision A).
2. **Scopes:**
   - an `if`'s then-branch gets what the condition proves when true;
   - its else-branch gets what it proves when false;
   - the right operand of `&&` and `implies` gets what the left proves when
     true.
3. **The right operand of `||` is not scoped** (what the left proves when
   false), though the rule computes it. A scope the checker accepts must be one
   the emitter lowers, and no lowering binds into `||` yet. `o is Some(v) || v
   > 0` stays `unknown name`.
4. **Lowering.**
   - The else-branch's bindings are declared at its top, as the then-branch's
     are.
   - An `is` receiver temp introduced in the right operand of an `&&`/`implies`
     is *declared* before the whole condition (`let __r1!: T;`) and *assigned*
     in place, inside the IIFE. The right operand stays lazy, and a then-branch
     binding that reads the temp finds it in scope.
5. **Book coverage (Decision B):** `spec/static-semantics.md` states the rule,
   and the narrowing guide shows the `&&` and negated forms.

**Consequences.** Programs the Settled spec already allows are accepted, and the
accepted-then-`tsc`-fails case compiles. No existing golden changed. Hover and
completion see a binding in the right operand of `&&`, since the checker now
types it there.

Proved by:
- the behavioural fixture `1654_is_narrowing_flow`, which covers both operands
  of `&&` narrowing a refined `Int`, the right operand of `&&`, a variant
  payload, `implies`, a negated else-branch, and a negated disjunction. It is
  rejected on `main`.
- negatives `1654_is_binding_not_in_or_rhs` and `1654_is_binding_not_after_if`;
- the rule's unit tests, one per row of the table.
