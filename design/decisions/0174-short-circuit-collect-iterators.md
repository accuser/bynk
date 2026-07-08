# 0174 — `List.traverseTry` / `parTraverseTry`, the short-circuit collect iterators

- **Status:** Accepted (v0.150)
- **Provenance:** the short-circuit-collect form named as a "later slice" by
  [ADR 0172](0172-list-collect-all-iterators.md) and
  [ADR 0173](0173-map-values-and-broadcast-collect-all.md) — the missing cell in
  the effectful-iteration matrix. Built directly on request.
- **Realises:** `xs.traverseTry(f)` / `xs.parTraverseTry(f)` — run a fallible
  `f: T -> Effect[Result[U, E]]` over a collection and **stop at the first
  `Err`**, returning `Effect[Result[List[U], E]]` (`Ok` of the collected values,
  or the first error). The fault-**propagating** counterpart to the
  fault-**gathering** `traverseAll`/`parTraverseAll` (ADR 0172).
- **Relates:** ADR 0172 (`traverseAll`/`parTraverseAll` — the collect-all pair
  whose machinery this mirrors), ADR 0173 (the `Query`/`Map` broadcast wiring
  reused here), ADR 0170/0171 (`forEach`/`parTraverse` — the discard pair), ADR
  0031 (effectful-context confinement), ADR 0156 (the editor surface).

## Context

The effectful-iteration matrix had one empty cell. Across the collect forms:

| | sequential | concurrent |
|---|---|---|
| discard | `forEach` | `parTraverse` |
| collect (gather all `Result`s) | `traverseAll` | `parTraverseAll` |
| **collect (short-circuit on first `Err`)** | **missing** | **missing** |

`traverseAll` returns `Effect[List[Result[U, E]]]` — *every* outcome, for the
caller to inspect. But the overwhelmingly common need — "apply `f` to each; if
any fails, stop and surface that failure; otherwise return the successes" — had
no direct form. A caller had to `traverseAll` then re-scan the `List[Result]` for
the first `Err`, doing the short-circuit by hand and paying to run every element
even after a failure (for the sequential case). This is the classic
`traverse`-over-the-`Result`-applicative, and its result type is
`Effect[Result[List[U], E]]`, not `Effect[List[Result[U, E]]]`.

This is a one-arm extension: the checker infers `f`'s `Effect[Result[U, E]]`
return exactly as `traverseAll` does (`check_kernel_fn_arg` + a `Result` peel,
factored here into `check_try_fn_arg`), and the emitter threads the `Result`
through a short-circuiting loop.

## Decisions

**A — New method names `traverseTry`/`parTraverseTry`, not a return-type overload
on `traverse`/`parTraverse`.** The design notes (§11) originally imagined
`traverse`/`parTraverse` as return-type-**overloaded** — one name dispatching on
whether `f` returns a plain value (`Effect[List[B]]`) or a `Result`
(`Effect[Result[List[B], E]]`, short-circuiting). The shipped language diverged:
`parTraverse` **discards** (`Effect[()]`, ADR 0171) and `traverse` is a *stdlib
free function* over plain values (`traverse(xs, f)`, not `xs.traverse(f)`).
Realising the overload vision now would mean (i) introducing **return-type
overload dispatch** — a new type-system mechanism the language has nowhere else —
and (ii) **migrating `traverse` off the stdlib** to a kernel method, a breaking
change to its call syntax, and (iii) making `parTraverse`'s result type depend on
`f`. New, distinct names avoid all three: additive, non-breaking, no new
mechanism, and consistent with the `…All` naming family that already
distinguishes fault-handling modes by suffix. **`Try`** evokes Bynk's `?`
operator (the `Result` short-circuit) and Rust's `try_*` combinators. *(The name
is the one open bikeshed — see the implementing PR; the mechanics are name-agnostic.)*

**B — Short-circuit on the first `Err`; `traverseTry` never runs later elements,
`parTraverseTry` cannot un-issue in-flight calls.** `traverseTry` awaits each
element in order and returns immediately on the first `Err` — later elements
never run. `parTraverseTry` issues all calls at once (they are already
in-flight), awaits them, then returns the **first `Err` in input order** — it
does not cancel siblings (matching `parTraverse`'s "cannot cancel already-issued
calls"), it only refrains from *starting* nothing new (there is nothing new to
start). This preserves the concurrency of `parTraverse` while giving a single
`Result` verdict.

**C — Same `check_try_fn_arg` inference and `argument_mismatch` gate as
`traverseAll`; effectful-context-confined; no new diagnostic.** The function must
return `Effect[Result[U, E]]` (a non-`Result` effect is
`bynk.types.argument_mismatch`, reused). On `List` the effectful-context gate
applies (`bynk.effect.fn_value_in_pure_context`); on the `Query`/`Map` broadcast
it does not, matching the `traverseAll` siblings.

**D — Reaches live connections via the same broadcast wiring (ADR 0173).** Both
are added to `is_query_op` + a `check_query_kernel_method` arm, so
`conns.traverseTry((c) => …)` on a `store Map[K, Connection]` routes through the
proven held-borrow path (each connection borrowed — `send` allowed,
`close`/transfer → `consume_on_borrow`).

## Consequences

- The effectful-iteration matrix is complete: discard (`forEach`/`parTraverse`),
  gather-all (`traverseAll`/`parTraverseAll`), and short-circuit
  (`traverseTry`/`parTraverseTry`) — sequential and concurrent throughout.
- No new diagnostic, no grammar change, no runtime change; two new kernel-method
  names (present on `List` and the `Query`/`Map` broadcast).
- No emission churn: net-new inline arms; every other kernel's lowering is
  untouched.
- The design-notes `traverse`/`parTraverse` return-type-overload vision is
  **explicitly not pursued** — the short-circuit capability is delivered under
  its own name instead.

## Tooling (ADR 0156)

- **Hover / Completion / Signature help:** both added to `LIST_METHODS`; the
  broadcast terminals are dispatch-only (as the other iterators are on the query
  side).
- **Semantic tokens / Formatter:** unchanged — ordinary method names.

## Alternatives considered

- **Return-type overloads on `traverse`/`parTraverse` (the design-notes vision).**
  Rejected for v1 — introduces return-type overload dispatch and a breaking
  `traverse` migration for a capability that distinct names deliver additively.
  Left open as a possible future unification.
- **No dedicated form — let callers scan `traverseAll`'s output.** Rejected — it
  forfeits sequential short-circuiting (every element runs even after a failure)
  and pushes a common, error-prone pattern onto every call site.
- **A single `Try`-suffixed sequential form only.** Rejected — the concurrent
  broadcast is exactly where "fail fast on the first bad connection" is wanted,
  so `parTraverseTry` ships alongside.
