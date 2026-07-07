# 0168 — A refined type inherits its base type's read-only kernel methods, producing base-typed results

- **Status:** Accepted (v0.143)
- **Provenance:** proposed in #561 (promoted from the reviewed long-form draft),
  resolving the design-review finding #537 (Bynk Language Design Review
  2026-07-05, §8 Language P1 #1).
- **Realises:** a method call whose receiver is a refined type resolves the base
  type's read-only kernel methods as a fallback after the type's own declared
  methods, with **base-typed** results — closing the one place a refined value
  did not already widen to its base.
- **Relates:** ADR 0046 (the string kernel), ADR 0048 (the numeric kernel), ADR
  0156 (the editor surface tracks the language), ADR 0063 (the enumerable
  kernel-method registry the LSP reads).

## Context

Refinement is the flagship feature — "make illegal states unrepresentable". Yet a
refined `String` dropped **every** string method: `fn shout(n: Name) -> String {
n.toUpper() }` failed with `bynk.types.method_not_found`, because the checker
dispatched the receiver `Name` down the user-declared-named-type path and never
reached the string kernel. Examples contorted around this — a plain `String`
parameter where a refined type belonged, or a value laundered back through
`"\(name)"` interpolation — so the feature punished exactly the users who adopted
it.

This was an **inconsistency**, not a missing capability. A refined value already
widens to its base everywhere else it is *read*: in assignability and call
arguments (`compatible` widens `Refined → Base`), in arithmetic and comparison
operands (the binary-op checker reads `.base()`), and in ordering keys
(`sortBy`/`min`/`max`). Method-call receiver dispatch was the sole hold-out. The
runtime representation of a refined value **is** its base (a branded type alias
erased at emit), so the base methods already applied bit-for-bit; only the checker
refused to look.

## Decisions

**A — Implicit inheritance, not an explicit widen step.** Read-only kernel methods
resolve directly on the refined receiver (`n.toUpper()`), rather than requiring a
hop (`n.widen().toUpper()`). This matches the widening a refined value already
gets in arithmetic, comparison, assignment, and ordering-key positions; a
mandatory hop here would be a lone inconsistency and would reintroduce the very
ceremony the finding removes. Nothing is silently lost because results are
base-typed (D-B).

**B — Base-typed results, uniformly; no preservation analysis.** `n.toUpper()` has
type `String`, never `Name`. The result is always the base type. This decouples
the increment from refinement propagation (the type system's largest open
question) and matches the shipped arithmetic rule. A later increment MAY upgrade
specific provably-preserving methods to return the refined type; because that
strictly narrows a return from base to refined, it is a compatible follow-on, not
a breaking change.

**C — The inherited set is the base's whole read-only kernel, stated as a rule.** A
kernel method is inherited iff it is **read-only** — it only reads its receiver
and returns a base or derived value. Every method in the String, numeric,
`Duration`, `Instant`, and `Bytes` kernels qualifies today (base values are
immutable; none mutate or narrow), so the inherited set is each base's entire
kernel. Stated as a rule, not a hand-maintained list, with the exclusion line
drawn now: a future method that *constructs* the refined type, or returns a value
whose validity depends on the predicate, is **not** auto-inherited. `Bool` has no
kernel, so a `Bool`-based refinement inherits nothing — correct.

**D — Declared methods win; the kernel is the fallback.** A refined type may
declare instance methods. If an author declares `toUpper` on `Name`, their method
wins; the inherited kernel is consulted **only** when the refined type's instance
table has no method of that name. Wired as a fallback *after* the instance-method
lookup, so a declared method can never be shadowed.

**E — Defer the `.widen()` companion.** With D-A + D-B, implicit use is not quiet —
results are already base-typed and `Refined → Base` assignability already holds, so
`let s: String = n` and passing `n` to a `String` param already work. `.widen()`
would be a second, near-redundant spelling of an identity widening. Deferred; if
real call sites show the implicit widening reads ambiguously, it is a cheap
additive follow-on. Recorded here so the deferral is on the record, not an
omission.

## The change

- **Grammar / AST:** none — the surface is an existing method-call form on an
  existing receiver type.
- **Checker (`bynk-check`):** in method-call dispatch, after the instance-method
  miss, a `Refined(base)` receiver routes `method`/`args` to the matching base
  kernel checker, returning its base-typed result; refined arguments already widen
  via `compatible`. The match is `Refined` only — `Opaque` keeps its
  `method_not_found`. `Bool` has no kernel and falls through to the same error. A
  stale comment claiming a refined value reaches the kernels "via `.raw`" (refined
  types have no `.raw`) is corrected.
- **Emitter (`bynk-emit`):** a `Refined(base)` receiver's inherited kernel call
  lowers through the **same** base-kernel helper as a plain base receiver — the
  emitted TypeScript is byte-identical (a refined value erases to its branded
  base), with no unwrap step. The declared-method-wins rule is mirrored: the
  kernel is routed only when the type declares no method of that name.
- **Runtime:** none — the kernels lower inline exactly as for base receivers.

## Tooling delta (ADR 0156)

- **Completion:** *changed* — `.`-member completion on a refined receiver now
  offers the base type's read-only kernel methods (in addition to the type's
  declared methods), via the enumerable registry's `methods_for` (ADR 0063) gaining
  a `Refined(base)` arm.
- **Signature help:** *changed* — signature help resolves for an inherited kernel
  method on a refined receiver, reusing the base kernel's parameter signature
  through the same `methods_for` mapping.
- **Hover:** *unchanged* — hover has no kernel-method-on-receiver signature path
  for base receivers either (it is identifier/declaration-based), so a refined
  receiver behaves identically to its base; there is no divergence to close and no
  regression. A dedicated kernel-method hover, for base and refined receivers
  alike, is a separate follow-on.
- **Semantic tokens:** *unchanged* — an inherited kernel call is an ordinary
  method-call token, already classified as such; no new token kind.

## Consequences

- A refined receiver resolves its base's read-only kernel methods with base-typed
  results; `fn shout(n: Name) -> String { n.toUpper() }` compiles and emits the
  same TypeScript as a plain `String` receiver.
- Declared methods on the refined type still win over the inherited kernel;
  opaque types are unaffected and still report `method_not_found`.
- No effect on any program that did not call a kernel method on a refined
  receiver — the change only turns a former `method_not_found` into a resolved,
  base-typed call.
