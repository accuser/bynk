<!--
LONG-FORM DRAFT of an increment proposal — transient, not a durable artefact.
Drafted here for line-anchored review before promotion (design/proposals/README.md
§"Drafting long-form proposals"). On acceptance this is promoted to a GitHub
issue from .github/ISSUE_TEMPLATE/increment-proposal.md (label `proposal`) and
this file is deleted; the issue is the sign-off artefact, `accepted` is the
approval to build. Do NOT pre-allocate the version or ADR number — both are
taken when the implementing PR lands.

Resolves the design-review finding in #537 (Bynk Language Design Review
2026-07-05, §8 Language P1 #1). Refs §4.1(4), §6.2(1), §6.3.
-->

# Refined types inherit the base type's read-only kernel methods

## Summary

- **Scope:** the **checker** (`bynk-check`) method-call dispatch, and the
  **emitter** (`bynk-emit`) method lowering. Grammar/AST unchanged, runtime
  unchanged. No new syntax.
- **Addresses:** a refined type cannot call any of its base type's kernel
  methods — `fn shout(n: Name) -> String { n.toUpper() }` fails with
  `bynk.types.method_not_found` (checker dispatches the receiver `Name` as a
  user-declared named type at `bynk-check/src/checker/calls.rs:1954` and finds
  no `toUpper` at `:2034`, never reaching the string kernel at `:1941`). This is
  the review's §8 Language P1 #1, filed as #537.
- **Realises:** a refined receiver resolves the base type's read-only kernel
  methods as a fallback after its own declared methods, producing **base-typed**
  results — closing the one place a refined value does *not* already widen to its
  base. `n.toUpper()` type-checks and returns `String`; no `.of`-round-trip and
  no `"\(n)"` interpolation laundering.

## Framing (why this is the language's to fix)

Refinement is the flagship feature — "make illegal states unrepresentable"
(`README.md`). The current gap teaches the exact opposite lesson: because a
refined `String` drops every string method, examples reach for a plain `String`
parameter where a refined type belongs, or launder a refined value back to a
usable `String` through interpolation — `let raw = "\(id)"`
(`examples/link-shortener/src/links.bynk:21`), and the `"flag:\(name)"` /
`"status:\(name)"` / `"link:\(code)"` key builders
(`examples/feature-flags/src/keys.bynk:16`,
`examples/uptime-monitor/src/status.bynk:8`,
`examples/link-shortener/src/codes.bynk:21`). Every such site is a user being
punished for adopting the feature the language is sold on.

Crucially, this is **not a new capability** — it closes an *inconsistency*. A
refined value already widens to its base everywhere else it is used:

- **Assignability / call arguments** — `compatible` widens `Refined → Base`
  (`bynk-check/src/checker.rs:1693`), so a `Name` is already accepted where a
  `String` param is expected.
- **Arithmetic and comparison operands** — the binary-op checker reads
  `.base()` on refined operands (`bynk-check/src/checker/expressions.rs:689`),
  and the rule is documented: "`a + b` of two refined `Int`s is a plain `Int`"
  (`site/src/content/docs/book/reference/refined-types.md:110`).
- **Orderable / numeric keys** — `sortBy`/`min`/`max` accept refined keys by
  widening (`site/src/content/docs/book/reference/types.md:180`).

Method-call *receiver dispatch* is the sole hold-out. The value is immutable
data whose runtime representation *is* its base (a branded type alias erased at
runtime — `bynk-emit/src/emitter.rs:6`), so the base methods already apply to it
bit-for-bit; only the checker refuses to look. The language should make dispatch
consistent with the widening it already performs.

## What exists today (grounded)

Method-call dispatch, `bynk-check/src/checker/calls.rs`:

- The `match recv_ty` at `:1902` routes **only** structural/base receivers to a
  kernel: `Ty::Base(String)` → `check_string_kernel_method` (`:1941`),
  `Ty::Base(Int|Float)` → `check_numeric_kernel_method` (`:1925`),
  `Duration`/`Instant`/`Bytes` likewise (`:1929`/`:1933`/`:1937`).
- A refined type is `Ty::Named { kind: NamedKind::Refined(base), .. }`
  (`bynk-check/src/checker.rs:127`) — it does **not** match any kernel arm, so
  it falls through to the named-type path at `:1954`, then agent-handler lookup
  (`:1971`) and the instance-method table (`:2034`). A refined `String` has no
  `toUpper` in its method table, so `:2034` emits `bynk.types.method_not_found`.
- The base kernels are already read-only and total. String
  (`bynk-check/src/checker/kernels.rs:1352`): `length`, `split`, `trim`,
  `toUpper`, `toLower`, `contains`, `startsWith`, `endsWith`, `replace`,
  `slice`, `indexOf`, `chars`, `concat`. Numeric (`:1124`): `toFloat`,
  `toString`, `abs`, `min`, `max`, `clamp` (+ `round`/`floor`/`ceil`/`truncate`/
  `isNaN`/`isFinite` on `Float`). `Duration`, `Instant`, `Bytes` carry their own
  small read-only kernels. `Bool` has no kernel.
- The comment at `calls.rs:1923` ("a refined value reaches them via `.raw`") is
  **stale**: `.raw` is an *opaque*-type surface
  (`bynk-check/src/checker.rs:132`); refined types have only `.of`/`.unsafe`
  (`site/src/content/docs/book/reference/refined-types.md:40`), and neither
  reaches a kernel method. Fix this comment as part of the increment.

## The surface

No new syntax. The observable change is that a refined receiver's read-only
kernel calls type-check where they are `method_not_found` today:

```bynk
type Name = String where NonEmpty

fn shout(n: Name) -> String { n.toUpper() }          -- was: method_not_found
fn key(n: Name)   -> String { "user:".concat(n) }    -- refined arg widens too
fn short(n: Name) -> Bool   { n.length() <= 8 }      -- length() : Int
```

Every result is **base-typed** — `n.toUpper() : String`, not `Name`. This is the
same rule the language already applies to refined arithmetic, so it needs no new
mental model: *a read that leaves the refined type returns the base type.*

## Decisions

**[DECISION A] Implicit inheritance vs. an explicit widen step (Recommended: implicit).**
The fork the finding raises. Option 1 — read-only kernel methods resolve
directly on the refined receiver (`n.toUpper()`). Option 2 — require an explicit
hop first (`n.widen().toUpper()` or `Name.raw(n).toUpper()`). Recommend **Option
1 (implicit)**: it is consistent with the widening a refined value *already* gets
in arithmetic, comparison, assignment, and ordering-key positions (see Framing),
so a mandatory hop here would be a lone inconsistency, and it reintroduces
exactly the ceremony the finding is removing. Nothing is silently lost because
results are base-typed (DECISION B): a refined value calling `.toUpper()`
visibly yields a plain `String`.

**[DECISION B] Result typing: base-typed vs. refinement-preserving (Recommended: base-typed).**
When `MaxLength(8)` calls `.toUpper()`, is the result `Name` or `String`?
Recommend **base-typed, uniformly** — the result is always the base type, with
**no** preservation analysis. This decouples the increment from §2.5.4
(refinement propagation, the type system's largest open question,
`design/bynk-type-system.md:972`) and matches the shipped arithmetic rule
(`refined-types.md:110`). A later increment may upgrade specific
provably-preserving methods to return the refined type under the §2.5.4 table;
because that is strictly narrowing a return type from base to refined, it is a
compatible follow-on, not a breaking change.

**[DECISION C] Inherited set: "read-only kernel" scope (Recommended: the whole current kernel of each base).**
"Read-only" = a kernel method that only reads its receiver and returns a base or
derived value. Every method in the String/numeric/`Duration`/`Instant`/`Bytes`
kernels qualifies today (base values are immutable; none mutate or narrow), so
the inherited set is each base's entire kernel. Recommend stating the **rule**,
not a hand-maintained list, so a future kernel method is inherited iff it is
read-only — and drawing the exclusion line now: a future method that *constructs*
the refined type or returns a value whose validity depends on the predicate is
**not** auto-inherited. `Bool`-based refinements inherit nothing (no `Bool`
kernel), which is correct.

**[DECISION D] Precedence vs. the refined type's own methods (Recommended: declared methods win; kernel is the fallback).**
A refined type may declare instance methods (`bynk-emit/src/emitter.rs:7`, "+
any user-declared methods"). If an author declares `toUpper` on `Name`, their
method must win; the inherited kernel is consulted **only** when the refined
type's instance table has no method of that name. Recommend wiring inheritance as
a **fallback after** the instance-method lookup at `calls.rs:2034` (try declared
methods first; on miss, if the receiver is `Refined(base)`, route to that base's
kernel; only then `method_not_found`). This preserves author intent and cannot
shadow a declared method.

**[DECISION E] `.widen()` companion — add now or defer (Recommended: defer).**
The finding offers `.widen()` "if implicit use is deemed too quiet." With
DECISION A + B, implicit use is *not* quiet — results are already base-typed and
`Refined → Base` assignability already holds (`checker.rs:1693`), so
`let s: String = n` and passing `n` to a `String` param work today without it.
`.widen()` would be a second, near-redundant way to spell an identity widening.
Recommend **deferring** it; if real call sites show the implicit widening reads
ambiguously, add `.widen()` as a cheap additive follow-on. (Kept as an explicit
decision so the deferral is on the record, not an omission.)

## The deltas (concretely)

- **Grammar / AST (`bynk-syntax`).** None, because the surface is an existing
  method-call form on an existing receiver type — no new production or node.
- **Checker (`bynk-check`).** In `check_method_call` (`calls.rs`), after the
  instance-method miss at `:2034`, add a refined fallback: when
  `recv_ty` is `Ty::Named { kind: NamedKind::Refined(base), .. }` and the type's
  instance table has no matching method, dispatch `method`/`args` to the base's
  kernel checker (`check_string_kernel_method` / `check_numeric_kernel_method` /
  `check_duration_kernel_method` / `check_instant_kernel_method` /
  `check_bytes_kernel_method`) keyed on `base`, returning the kernel's
  (base-typed) result. Refined-arg positions already widen via `compatible`
  (`checker.rs:1693`), so kernel argument checking needs no change. Update the
  stale `.raw` comment at `:1923`. Record the method reference edge the same way
  the kernel arms do, for LSP indexing.
- **Emitter (`bynk-emit`).** In `lower_method_call` (`emitter/lower.rs:848`),
  route a `Refined(base)` receiver's inherited kernel call through the **same**
  base-kernel lowering the base receiver uses (`lower_string_kernel` at `:1429`,
  `lower_numeric_kernel` at `:1399`, etc.). Refined values erase to their branded
  base representation (`emitter.rs:6`), so the emitted call is identical to the
  base receiver's — `n.toUpper()` → the same inline `String` lowering as a plain
  `String` — with no unwrap/`.raw` step.
- **Runtime.** None — no `runtime.ts` change; the kernels lower inline exactly as
  they do for base receivers.

## Risks & mitigations

- **Silent widening surprises a reader** (`n.toUpper()` looks like it stays
  `Name`) → results are base-typed and this mirrors the already-shipped
  arithmetic rule; the reference and the refined-types tutorial state it
  explicitly (Docs delta). DECISION E keeps `.widen()` in reserve if practice
  shows it reads too quietly.
- **Shadowing a future/declared method** → DECISION D makes the kernel a strict
  fallback *after* the declared-method table, so a declared method always wins.
- **Coupling to refinement propagation (§2.5.4)** → DECISION B returns base
  types only, taking no position on preservation; a later preserving-return
  upgrade is compatible (narrowing base→refined return).
- **Opaque types must stay unaffected** — they deliberately do **not** widen
  (`checker.rs:132`). Mitigation: the fallback matches `NamedKind::Refined`
  only; `Opaque` keeps its current `method_not_found`, and a fixture asserts it.

## Docs delta

- **Reference:** `site/src/content/docs/book/reference/refined-types.md` — new
  "Inherited base methods" section: read-only kernel methods resolve on the
  refined receiver and return the **base** type; declared methods take
  precedence; cross-link the base kernels in
  `site/src/content/docs/book/reference/types.md`. Tighten `refined-types.md:110`
  so the "not preserved through arithmetic" note also covers kernel-method
  results (same base-typed rule).
- **Guide / tutorial:** `site/src/content/docs/book/tutorials/04-refined-types.md`
  gains a short "calling base methods on a refined value" beat — a refinement of
  an existing concept, so a recipe/reference update, not a new "Understand"
  on-ramp.
- **Diagnostics:** `bynk.types.method_not_found` on a refined receiver should,
  where cheap, hint the base kernel; note the wording change in
  `site/src/content/docs/book/reference/diagnostics.md` if the message moves.
- **Changelog + version history:** advance the `spec/index.md` currency banner
  (currently v0.142) and `spec/appendix-version-history.md` to the version this
  ships as; add a `reference/changelog.md` entry.
- **Roadmap:** #537 closes; remove the "refined types currently drop base
  methods until #537 lands" caveat that #538 (Language P2, spec-consolidation
  bullet) tracks.
- **Spec:** `site/src/content/docs/book/spec/static-semantics.md` states the
  refined-receiver kernel-dispatch and base-typed result rule normatively.

## Tooling delta (ADR 0156 — silence is an oversight)

- **Hover:** changed — hovering an inherited kernel method on a refined receiver
  shows the base kernel method's signature (base-typed result), reusing the base
  kernel hover.
- **Completion:** changed — `.`-completion on a refined receiver now offers the
  base type's read-only kernel methods (after the type's declared methods), where
  today it offers only declared methods.
- **Semantic tokens:** unchanged, because inherited kernel calls are ordinary
  method-call tokens already classified as such — no new token kind.
- **Signature help:** changed — signature help resolves for an inherited kernel
  method on a refined receiver, reusing the base kernel's parameter signature.

## Done when

- A refined receiver resolves its base's read-only kernel methods with
  base-typed results; `fn shout(n: Name) -> String { n.toUpper() }` compiles.
- Declared methods on the refined type still win over inherited kernel methods
  (DECISION D); an opaque type is unaffected and still reports
  `method_not_found` (DECISION opaque risk).
- Emitted TypeScript for an inherited call is identical to the base receiver's
  and passes `tsc`.
- Fixtures (next free indices) cover: String inherit (`toUpper`/`length`/
  `slice`), numeric inherit (`abs`/`clamp`/`toString`), a declared-method-wins
  case, an opaque-still-rejected case, and a `Bool` refinement (no kernel).
- Docs current per the delta above; the four tooling surfaces stated.
- Version bump (`scripts/bump-version.sh`) — this is a language increment.
- A new ADR records DECISIONS A–E; its number is taken when the implementing PR
  lands, which closes the promoted proposal issue (`Closes #<proposal>`).
