# Runtime semantics — emitted programs do what the spec says

- **Status:** Slicing. Direction was settled by the merge of
  [#1681](https://github.com/accuser/bynk/pull/1681) (`bec61020`). No slice is accepted yet;
  each is an ordinary increment proposal on the spine, and the four front-loaded ADRs (§5)
  land with their first slice.
- **Realises:**
  - the book's normative spec (`site/src/content/docs/book/spec/static-semantics.md`,
    `emission.md`, `type-system.md`);
  - the Settled sections of `design/bynk-type-system.md`;
  - the compilation-model promise that output is `tsc --strict`-clean (`compilation-model.md:91,98`).
- **Posture:** Feature track per [ADR 0076](../decisions/0076-feature-track-posture.md),
  run GitHub-native per [ADR 0167](../decisions/0167-feature-tracks-run-github-native.md).
  It qualifies on all three axes (§2). In particular it is a **safety boundary**:
  - a refinement regex runs on attacker-controlled input;
  - agent-state persistence is the durable-state surface 1.0 Gate 3 (#539) builds on;
  - the atomic-handler guarantee (ADR 0109) is currently violated on the bundle target (§3.2).
- **What's already true:** the checker's core is sound under adversarial probing. The
  [2026-10-01 review](../reviews/2026-10-01-language-implementation-review.md) §1.5 lists
  refinements, narrowing scope, nested exhaustiveness, variance, opaque encapsulation, effect
  discipline, recursive types through JSON and string semantics as all holding. The defects are
  almost all at the **emission** boundary, and in what the gates can see. Spine
  [#1648](https://github.com/accuser/bynk/issues/1648).

## 1. The theme

The compiler's gates certify two things about a positive program:
- its emitted TypeScript **type-checks** (`bynkc/tests/tsc_verify.rs`, `tsc --strict`);
- that TypeScript **matches a blessed golden** (`bynkc/tests/e2e.rs`).

Nothing certifies that it **behaves** the way the spec says a Bynk program behaves. A golden
blesses whatever was emitted. Fixtures `139_agent_state_zero_option` and `155_state_sum_machine`
have blessed an agent-state reload fault since `5c957082` (26 June 2026). 71 positive fixtures
carry `suite`s that are type-checked but, apart from four, never run. Running them all during
this track's settling found one that can never pass and one that fails pending triage
(§3.5).

The review ran about 200 small programs against the spec and found the gap populated:
- agent state holding an enum or `Option` faults on every load after its first write;
- `==` is reference equality on records, sums, `List` and `Option`;
- a regex guard that misses ambiguous alternation, on request input;
- `String.replace` expanding `$&`;
- a class of programs that are accepted but emit TypeScript `tsc` rejects;
- several spec rules the checker under- or over-enforces.

Settling research (§3) added more defects of the same kind, each reproduced:
- **A refused commit leaks.** On the bundle target, which `bynkc test` also uses, a handler whose
  commit is refused by an invariant still mutates stored state.
- **The regex guard pins a wrong case.** It blesses `(a{2,3})+` as "safe" in a test; it is
  exponential.
- **Workers Durable Object calls bypass the boundary codec.**
- **WebSocket `Int` open parameters arrive as strings.**
- **Unknown type names in handler signatures are not diagnosed.**
- **`store Set[record]` is accepted and then fails `tsc`.**

**End state when this track retires:**
- every defect in §4 is fixed, or settled by a recorded decision;
- the gates assert **runtime behaviour** for the semantics this track touches (§3.5), so the
  class cannot silently return.

## 2. Why a track (the ADR 0076 trigger)

- **Multi-increment.** About 20 independent defects and decisions across the checker, the
  emitter, the runtime and the test infrastructure. They share a theme and a gate, but no single
  delete-on-merge proposal can carry them.
- **Surface not yet settled.** Four of them change what a program *means*:
  - what `==` means (§3.1);
  - the on-disk shape of agent state (§3.2);
  - which regexes are admissible (§3.3);
  - what an `Int` is (§3.4).

  Each needs an ADR before it is built.
- **Security/safety boundary.** §3.3 is a denial-of-service surface on request input. §3.2 is
  durable data and the atomic-commit guarantee. §3.1 decides whether agent invariants written
  in the spec's own idiom (`status == Paid implies …`) can fire once state is decoded (§3.1).

## 3. Design questions (settled)

Each question below was investigated during settling. The answer is argued from evidence in the
tree, cited to file:line, and every behavioural claim was reproduced. All five were settled by
the merge of #1681. Two sub-points were deliberately left to the slice that implements them, and
are marked as such.

### 3.1 — What does `==` mean on a record, a sum, a `List`, an `Option`? — SETTLED: structural

**Today.** `lower_bin_op` (`bynk-emit/src/emitter/lower.rs:4812-4834`) special-cases `Bytes`
(`__bynkBytesEqual`, [ADR 0142](../decisions/0142-bytes-primitive.md) D4) and lowers every
other type to host `===` (`ts_binop`, `emitter.rs:5131`). So:
- `Some(1) == Some(1)` is `false`, and `Some(1) != Some(1)` is `true`;
- `P { x: 1 } == P { x: 1 }` is `false`;
- `[1, 2] == [1, 2]` is `false`.

Nullary variants are equal only in-process: `St.On` is a shared singleton, but `deserialise_St`
builds a fresh `{ tag: "On" }`, so a decoded `On` is not `== On`. ADR 0142:54/:127 call
whole-record `==` reference equality "today; changing that is a separate decision". That
decision was never taken. The Settled type-system spec §2.3.5 already specifies structural
equality for sums.

**Why it matters more than an edge case:**
- **The language contradicts itself.** `x is Paid` is structural (it lowers to
  `x.tag === "Paid"`, `lower.rs:2928`); `x == Paid` is not.
- **Invariants would silently stop firing.** The spec's own invariant idiom,
  `status == Paid implies paymentRef.isSome()` (fixture 222; type-system spec :1642; book
  `reference/agent-invariants.md:26,28,89`), lowers to `s.status === OrderStatus.Paid`.
  - **Today** the question is masked: a stored enum faults on load (#1649) before any invariant
    runs. On the bundle target, `InMemoryStorage` (`runtime.ts:128-137`) hands back the stored
    reference, so `===` against the singleton still holds.
  - **Once §3.2 decodes state** into fresh objects, which it does on both targets by
    construction, `status` is never `===` the `OrderStatus.Paid` singleton. The antecedent can
    then never be true, so the **invariant never fires**. The same goes for
    `filter(r => r.status == Pending)`.
  - Whether workerd's own storage already returns copies today was not verified during
    settling, and the argument does not depend on it.
- **Tests already assume structural equality.** Test stub argument matching uses a third
  semantics, `JSON.stringify` comparison (`test_runtime/stub.ts:1-4`). That makes every `Map`
  equal and NaN equal to NaN.
- **The checker allows `==` on more than §2.3.5 permits.** It rejects only `Stream` and a
  *top-level* `Connection` (`checker/expressions.rs:857-919`). It accepts functions, `Effect`,
  `Option[Connection[F]]` (contradicting §2.3.5's recursive rule), and bare type parameters.
  Opaque-over-`Bytes` emits `===` because `base()` does not see through `Opaque`.

**Options:**
- **(A) Derive structural equality.** This is what §2.3.5 specifies, what the invariant idiom
  needs, and what `is` and stub matching already do.
- **(B) Reject `==` on non-primitive types.** It is honest, but it breaks the spec's own Settled
  examples (every `status == Paid` in fixtures 222/223/245, the type-system spec, the site's
  invariants page and the design notes). It also loses `Some(x) == y`, record `expect`s and
  History `old == new`, and still leaves stub matching needing structural equality.

**How A is built.** A type-directed `equals_T`, generated beside the per-type codecs
(`serialisation.rs`), founders on generics. Bynk generic functions are **not** monomorphised; they
emit as TS generics (`same<T>`), so `==` on a `T` has no concrete type to dispatch on. It would
need dictionary passing.

The in-memory representation describes itself, so a **runtime structural walker** needs no type
information:
- `===` for primitives (keeping IEEE behaviour: `NaN != NaN`, `-0 == 0`);
- content comparison for `Uint8Array`;
- element-wise for arrays;
- size plus per-key for `ReadonlyMap`;
- `tag` then fields for plain objects;
- `undefined` for unit.

The lowering is a three-way, operand-typed dispatch at the one choke point, `lower.rs:4812`, the
same shape as `Div` and `Bytes` today:
- statically primitive operands (including refined/opaque over non-`Bytes`) → `===`;
- `Bytes`, **including opaque-over-`Bytes`** → `__bynkBytesEqual`;
- everything else → `__bynkEq`.

**Prior art.** Typed functional languages with structural `==` exclude functions either at
compile time (Roc's `Eq` ability, Rust's opt-in `PartialEq`) or at runtime (Elm's crash on
function `==`, OCaml's `Invalid_argument` from `=`). The checker route is the better one, and
it is what §2.3.5 already describes ("the compiler errors at any `==` site involving the type").

**Recommendation: A**, as:
1. `__bynkEq` in the runtime, with the dispatch above.
2. A recursive *equality-supporting* predicate in the checker, replacing the top-level-only
   check. It rejects `Fn`, `Effect`, `Query`, `Stream` and held resources **anywhere inside**
   `Option`/`List`/`Map`/`Result`/records/sums. Bare type parameters stay allowed, because the
   walker handles them.
3. Stub argument matching switches to `__bynkEq`, so the language has one equality.
4. `is_keyable` (`distinct`/`groupBy`/`Map` keys/`@indexed`) is **unchanged** in this track.
   Structural keys are a follow-on that A enables, not part of it.

**Cost.** Small to medium. The pieces are a ~40-line runtime helper, one dispatch edit, a
~40-line checker predicate with negative fixtures, the stub swap, and the ADR. Golden churn is
limited to `==` sites on non-primitive operands (fixtures 222/223/245 and `spec/emission.md:163`).

**Sequencing with §3.2.** Once §3.2 decodes state into fresh objects, `bynkc test` loses the
reference aliasing that currently masks this question. Every `status == Paid` invariant in a
test would then start failing. **#1652 must land with or before #1649.**

### 3.2 — What shape does agent state take on disk? — SETTLED: the wire shape

**Today.** There is exactly one storage key per Durable Object, `"state"`, holding the whole
state record as the emitter's in-memory TS representation (`commitState`, `emit.rs:5417-5425`:
`storage.put("state", s)`). `loadState` (`emit.rs:5127-5190`) merges `{ ...zero(), ...stored }`
and calls `__rehydrate<Agent>State` (builder `emit.rs:4904-5029`). That function launders each
value to `JsonValue`, validates it with the **wire** deserialiser, and **discards the result**.

Wherever the in-memory and wire shapes differ, every load after the first commit throws
`RehydrationViolation`:
- sums, enums, `Option`, `Result` (`tag` vs `kind`);
- `Bytes` (`Uint8Array` vs base64);
- value-level `Map` (JS `Map` vs an entries array);
- non-finite `Float`.

Settling research reproduced **16 of 23** storable shapes failing under `bynkc test`, at any
depth, in `Cell`, store `Map`, `Cache`, `Log` and `@indexed` records alike. Structured clone is
**not** the cause: it preserves all of these. The validator simply expects a shape storage never
holds.

There are **four** sites that touch `"state"`, not two:

| Site | Location |
|---|---|
| `loadState` | `emit.rs:5127-5190` |
| `commitState` | `emit.rs:5417-5425` |
| the transition-predicate `__prior` read | `emit.rs:5391-5412` (raw, unvalidated) |
| the History-property driver's `__load` | `emit.rs:6217` (raw) |

**A second, independent defect: a refused commit leaks.** Every handler begins
`const __state = { ...(await this.loadState()) }`, which is a *shallow* copy. On the bundle
target, `InMemoryStorage` (`runtime.ts:128-151`) returns the stored object itself, so a store
write such as `__state.items[k] = v` mutates the record still held in storage. If the commit is
then refused, the write has already happened. Reproduced:
1. `add("a")` commits.
2. `add("b")` is refused by `invariant small: total <= 1`.
3. `count()` returns **2**.

That breaks [ADR 0109](../decisions/0109-handler-atomic-commit.md)'s atomic-handler guarantee on
every bundle deployment and in every `bynkc test` run. It also makes `bynkc test` structurally
unable to observe clone semantics (the masking noted in §3.1).

**Options:**
- **(A) Store the wire shape.** Encode on commit with the boundary serialiser; decode on load
  *into* state.
- **(B) Validate the in-memory shape** with a new, third codec family.

**[ADR 0124](../decisions/0124-rehydration-validation-and-migration.md) against each:**

| ADR 0124 | A | B |
|---|---|---|
| D1 "the **same** validator the HTTP/queue/event boundaries use" | honours it literally | violates it in spirit |
| D2 (`RehydrationViolation`) | unaffected | unaffected |
| D3 (no coercion) | unaffected | unaffected |
| D5 (deferred migrations) | unaffected | unaffected |
| D4 (zero-then-stored merge) | needs new mechanics: `zero()` is in-memory shape and `stored` is wire shape, so load becomes per-field (`stored.f === undefined ? zero.f : decode(stored.f)`), and the "added field takes its default" proof is re-run | survives unchanged |

**What #539 needs.** A stored schema fingerprint, a `migrate` transform, and rename detection.
Under A:
- **The fingerprint already exists in kind:** `bynk-check/src/contract.rs`'s `canon_type`,
  documented as "the canonical form of a type *as it appears on the wire*", plus `contract_hash`
  (`:326`). The fingerprint becomes `hash(sorted store fields × canon_type)` and describes the
  bytes on disk exactly.
- **A `migrate` transform can be written against `JsonValue`,** a normative, documented shape
  (spec §7).

Under B, the disk format is emitter internals (`tag`, `Uint8Array`, JS `Map`) with no canonical
form, and a fingerprint would have to track every representation change.

**Legacy data.**
- **No working agent changes on disk under A.** For every type that works today, A's on-disk
  bytes are **identical** to today's, because the in-memory and wire shapes coincide.
- **Only bricked agents hold the legacy shape.** The only records in a different shape belong to
  agents that are *already* faulting on every load. A tolerant loader would recover data, not
  keep anything working.
- **There is no known external deployment.** The project is pre-1.0, and ADR 0124's own
  rejected alternative (c) cites "no external persisted corpus".
- **A tolerant loader would have to be type-directed.** A generic `tag`→`kind` rename corrupts
  any record with a field named `tag`. Threading a legacy flag through the shared `deserialise_*`
  would risk widening the HTTP boundary to accept `{tag:…}`.

**Cost.**
- **Load:** no change. `__rehydrate` already runs the full deserialiser over every value on
  every handler call and throws the result away; A keeps it. (ADR 0124's "one validation pass
  per cold load" consequence is wrong: it is per handler call. The new ADR corrects it.)
- **Commit:** gains one `serialise_*` walk per codec-able value, O(state). That is the same
  order as the V8 serialisation `put` already performs.

**Recommendation: A, with no tolerant loader.** A changelog note says agents bricked by #1649
must have their storage cleared. The decode returns fresh objects, which closes the leak as a
side effect on both targets. The leak still gets its own regression fixture (§4, S0).

On-disk layout under A (still one `"state"` key):

| Field kind | Stored as |
|---|---|
| `Cell[T]` | `wire(T)` |
| store `Map[K, V]` | `Record<String(k), wire(V)>` |
| `Cache` | `Record<String(k), { v: wire(V), exp }>` |
| `Log` | `[{ t, v: wire(T) }]` |
| `Set`, held maps, `@indexed` posting lists | unchanged (already string-shaped) |

Carried into S0's proposal (#1649), to decide there: a non-finite `Float` now fails **at commit**, so nothing persists
and the agent is not bricked. But it surfaces as an untyped `Error("non-finite Float at
boundary")` (`serialisation.rs:2066-2110`). Should it be an `InvariantViolation`-class fault?

### 3.3 — Which regexes may a `Matches` refinement use? — SETTLED: an automaton ambiguity check

**Today.** The `catastrophic_regex` guard (`bynk-check/src/checker/refinements.rs:903-1051`,
`has_nested_unbounded_quantifier`) is a hand-rolled scan that rejects an unbounded quantifier
applied to an atom that already contains one. It cannot use `regress`'s AST, because regress
0.12 keeps `parse`/`ir` private. Emitted code runs `new RegExp("^(?:" + pat + ")$")` with no
flags (`lower.rs:6104`) on request input. The workers route entry calls `handlers.T.of(raw)`.
The guard knowingly misses ambiguous alternation and pins that in a test citing a closed issue
(#724).

**Settling research measured more than the review did** (Node 24 / V8 13.6, anchored as
emitted, input `u^n + "!"`):

| Pattern | Guard today | Behaviour |
|---|---|---|
| `(a\|a)+` | passes | exponential (25.6 s at n=28 in the review's end-to-end probe) |
| `(\d\|\d\d)+`, `(a\|aa)*`, `(\w\|\d)+` | passes | exponential |
| `(a{1,2})+` | passes | exponential (145 ms at n=30, ×6 per +4) |
| **`(a{2,3})+`** | **passes, pinned "safe" by `allows_safe_patterns`** | **exponential**: 6 ms at n=40, 1.5 s at n=60. The pin and its comment ("finite *inner* bound cannot explode") are wrong |
| `(foo\|foobar)+` | passes | **linear** (3 ms at n=30,000). {foo, foobar} is uniquely decodable. The review, #1651 and the guard's own comment wrongly list it as exponential |
| `\d*\d*` | passes | polynomial (391 ms at n=20,000) |

**Options:**
- **(R1) Reject alternation under an unbounded quantifier unless the branches are first-character
  disjoint.**
  - *Unsound:* bounded quantifiers inside a loop are choice points too, so `(a{1,2})+` and
    `(a?a)+` pass.
  - *Over-strict:* it rejects the linear `(foo|foobar)+`.
  - *Breaks the stdlib:* it rejects the first-party `LocaleTag` pattern
    (`bynk-check/src/firstparty/bynk.locale.types.bynk`), whose `-(A|B)` branches overlap on
    `[0-9]` but resynchronise at each `-`.
- **(R2) A linear-time engine at runtime.**
  - V8's linear engine (`/l`) needs a flag workerd does not set: `new RegExp("a","l")` throws
    `SyntaxError: Invalid flags` in workerd 1.20260930.
  - A shipped WASM RE2 or Rust `regex` would cost most of the Worker bundle budget, and would
    drop lookbehind and backreferences, which the corpus uses (`[a-z]+(?<=ing)`).
  - A synchronous `RegExp.test` cannot be timed out.
- **(R3) An automaton ambiguity check at compile time.** This is the standard result: Weideman,
  van der Merwe, Berglund & Watson, *Analyzing matching time behavior of backtracking regex
  matchers* (CIAA 2016), building on Allauzen/Mohri/Rastogi's NFA ambiguity tests. It is the
  approach behind rxxr2 and recheck. The steps:
  1. Parse to an AST (extending the existing scanner).
  2. Expand bounded repeats as **nested** optionals (`x{5,8}` → `xxxxx(x(x(x)?)?)?`; the flat
     form invents ambiguity), over-approximating `{n,m}` as `{n,}` above about 64.
  3. Build the Glushkov NFA and the product automaton A×A.
  4. **Exponential ambiguity** exists iff some strongly connected component holds both a diagonal
     pair (p,p) and an off-diagonal pair (p,q). That is an error.
  5. **Polynomial ambiguity** exists iff there is a path (p,p,q)→(p,q,q) in A³ between two
     cyclic states.

  Soundness obligations:
  - lookaround bodies are analysed as their own unanchored patterns;
  - a backreference under an unbounded quantifier is rejected;
  - a nullable body under an unbounded quantifier is rejected;
  - NFA size is capped (reject above the cap).

**Compatibility cost of R3, measured over every `Matches(` in the repo** (32 strings, 27
parseable patterns, across `.bynk`, docs and Rust tests):
- the only newly rejected pattern is `(a|a)+`, in the review document;
- every positive fixture, doc, example and first-party pattern passes, including `LocaleTag`.

A Python prototype is about 200 lines, so the Rust is an estimated 400–500 lines in
`bynk-check`.

**Recommendation: R3.**
- **Exponential ambiguity** is an error.
- **Polynomial ambiguity** is an error, downgraded to a warning when a `MaxLength` predicate
  precedes the `Matches` in the same refinement. Predicates short-circuit in source order
  (`refined_check_as_bool`, `lower.rs:6078-6105`), so a length bound caps the polynomial.
- The emitter additionally **hoists length predicates ahead of `Matches`**.
- `has_nested_unbounded_quantifier` stays as a cheap first pass with its existing, clearer
  diagnostic.
- `allows_safe_patterns` drops the `(a{2,3})+` pin, and `does_not_flag_known_deferred_*`
  becomes positive assertions minus `(foo|foobar)+`.

### 3.4 — What is an `Int`? — SETTLED: the JS safe-integer domain, enforced at every entry

**Today, no document defines the domain, and the three entry points disagree:**
- **The lexer** accepts any `i64` (`bynk-syntax/src/lexer.rs:603`). `9223372036854775807` is
  emitted verbatim and rounds.
- **The wire** accepts any `Number.isInteger`, including `1e300` (`serialisation.rs:1027`,
  `:1792`, `:2481`; `emit.rs:311`; `lower.rs:6081`).
- **`Int.parse`** applies `isSafeInteger` after `trim()` + `Number()` (`lower.rs:1886-1897`).
  So `"0x10"` gives `Some(16)`, `"1e3"` gives `Some(1000)`, `" 7 "` gives `Some(7)`, and `"-0"`
  gives `Some(-0)`. This contradicts static-semantics:124-131 ("full-string … leading/trailing
  garbage is `None`").

`Float.parse` has the same laxity (`" 7 "`, `"0x10"`, `"0b11"`, `"5."` all accepted).

[ADR 0048](../decisions/0048-combinators-as-kernel-methods.md) already calls `isSafeInteger` "the honest
bound". [ADR 0319](../decisions/0319-positive-nonnegative-inrange-fold-declined.md):31-36 acknowledges the
i64-vs-2⁵³ gap (its `emitter.rs:3979` reference is stale; the mapping is now `emitter.rs:4384`).
[ADR 0042](../decisions/0042-operand-typed-division.md) makes arithmetic host-defined and
boundaries guarded.

Settling found the type **unsound**, not just imprecise:
- `5 / 0` and `0 / 0` produce `Infinity`/`NaN` as `Int` values (`lower.rs:4810`).
  `Json.encode` writes them as `null`, and the consumer's deserialiser then rejects them.
- `round`/`floor`/`ceil`/`truncate` (`lower.rs:3689-3694`) turn a non-finite `Float` into an
  `Int`.
- **A WebSocket `on open (room: Int)` parameter is never parsed.** The runtime result below was
  reproduced by settling research; the mechanism is confirmed in the generated
  `workers/<ctx>/index.ts`, which passes `url.searchParams.get("room")` on unparsed.
  `url.searchParams.get("room")` reaches the agent as a string cast `as number`, so `room + 1`
  evaluates to `"51"`, and `?room=05` routes to a different agent than `Room(5)`. HTTP path
  params have the `is_string_constructible` check (`context_checks.rs:3381`); WebSocket open
  params have none.

**Options:**
- **(i) Safe-integer domain ±(2⁵³−1)**, enforced at every entry.
- **(ii) i64 via `bigint`.** `JSON.stringify` throws on `bigint`, so the wire format and every
  consumer would break. Arithmetic, `length()` and `Duration` interop all change, and it is
  slower.
- **(iii) Status quo, documented.** Leaves the type unsound.

**Recommendation: (i).**
- **Literals:** a new `bynk.lex.integer_out_of_safe_range` diagnostic, also applied to
  `InRange` bounds.
- **The wire:** every boundary check uses `Number.isSafeInteger`.
- **Strict grammars:** `Int.parse` uses `^[+-]?[0-9]+$` then `isSafeInteger`, normalising
  `-0`. `Float.parse` uses `^[+-]?([0-9]+(\.[0-9]*)?|\.[0-9]+)([eE][+-]?[0-9]+)?$` then
  `isFinite`.
- **`Int` division by zero traps.** It becomes a runtime fault like an invariant violation,
  rather than producing a non-integer `Int`.
- **WebSocket open params** follow HTTP path params: reject a non-stringy type in the checker.
- **Arithmetic overflow** on `+ - *` stays **documented imprecision**, consistent with ADR 0042's
  "arithmetic host-defined, boundaries guarded". The boundaries are then the guarantee: an
  out-of-range value can never be accepted from outside, or written out, undetected.

Carried into S8's proposal (#1657), to decide there: Float→`Int` conversions of a non-finite or >2⁵³ value. Either
(a) return `Option[Int]`, which is a signature change to four kernel methods, or (b) trap, the
same as `/0`. Leaning (b), for consistency with division.

### 3.5 — How do the gates assert runtime behaviour? — SETTLED: opt-in run markers on positive fixtures

**Today:**
- 71 positive fixtures already contain `suite`s. `tsc_verify.rs:211-340` type-checks them in
  one batched pass, but only **four** are ever executed (`1426_*` ×2,
  `int_binding_arithmetic_behaviour.rs`; `804`/`807`,
  `adapter_flattened_capability_stub_behaviour.rs`).
- Settling ran all 71 under `bynkc test`. 68 pass and 3 fail:
  - `107_test_with_assertion_failure` fails **deliberately**.
  - `1402_stub_fails_and_single_outcome_sequence` fails **all three** cases. Its suite-level
    `stub Vault.open() fails` makes every `box.call()` throw, so this golden blesses a suite that
    can never pass.
  - `385_system_wire_rejection` fails one case. The fixture is `target.txt = workers`, which
    `bynkc test` ignores; this needs triage.
- The existing `bynkc test` runner already catches #1649. A `139` copy with a write-then-read
  case fails with `RehydrationViolation`.
- `store_behaviour.rs:948-980` missed #1649 because it seeds storage by hand with scalars and
  never writes then reloads through the handlers.

**The design:**
- **Marker.** A project-form positive fixture opts in with an `expected_run.txt`:

  ```
  passed=3 failed=0
  fail <case name>        # optional, for deliberate failures such as 107
  ```

  A suite-bearing fixture without the marker is not executed. That keeps 1402 and 385 out until
  triaged, and keeps the class opt-in and reviewable.
- **Runner.** One new test, `bynkc/tests/behaviour_fixtures.rs`. For each marked fixture it
  runs `bynkc test <dir> --output <tmp>/<name>/out --format json`, parses the pinned JSON
  document (`test_json.rs:22-35`), compares `passed`/`failed`, and **requires at least one
  case**. (`bynkc test` exits 0 with "no test declarations found", `test_runner.rs:304-314`.)
- **Cost.** About 1.0–1.3 s per fixture single-threaded. The initial ~15-fixture priority set
  takes about 20 s.
- **Scaling.** If the class grows past about 40 fixtures, switch to a batched runner: compile
  all with `--no-run`, one root `tsc -p`, then per-fixture `node`. That measured about 14 s for
  all 71.
- **Gating.** `require::is_required("BYNK_REQUIRE_TSC")`, skipping loudly otherwise, as the
  sibling `*_behaviour.rs` tests do.
- **Target.** Bundle only. `bynkc test` has no `--target` flag. A workerd leg (about 4–5 s per
  fixture warm, via `wrangler dev`, as `workers_runtime_smoke.rs` does) is opt-in future work,
  except for one new agent-state smoke required by S0.
- **Probe.** `fixture_kinds` gains `run=<count>` (and `warnings=`, which it omits today). It is
  a trend probe, so no gate breaks; refresh with `cargo xtask greenfield-status --apply`.
- **Harness fidelity.** `InMemoryStorage` must clone on `put`/`get` (a V8-serialise round trip
  via `structuredClone`), so `bynkc test` observes what workerd does. Otherwise it keeps hiding
  §3.1-class defects. Under §3.2's option A, agent state is stored encoded anyway, but the clone
  makes the harness honest for every other use.

The initial priority set:
- reload round trips: 139, 155, 138, 140, 224, 225, 228, 232, 241;
- commit detection: 1196, 1197;
- codecs: 212, 373;
- string and numeric kernels: 208, 210;
- refined `.of`: 02–10;
- narrowing: 164–166;
- events: 961, 966;
- a new equality fixture.

## 4. Candidate slice decomposition

Each slice is an increment-proposal sub-issue of #1648. **Order matters in two places:**
- S3 (`==`) lands with or before S0 (state shape), per §3.1's sequencing note.
- G0 (behavioural fixtures) lands early, so each later slice can add its runtime proof.

**Correctness**

| Slice | Issue | Content | Settled by |
|---|---|---|---|
| **S0** | #1649 | Agent state stored in the wire shape. Four `"state"` sites; per-field decode with D4's default; refused-commit leak fixture; reload fixture over every store kind × sum/`Option`/`Result`/`Bytes`/value-`Map`/`List[enum]`; new workerd agent smoke | §3.2 |
| **S1** | #1650 | `String.replace` → `replaceAll(a, () => b)` | — |
| **S2** | #1651 | Automaton ambiguity check for `Matches`; length-predicate hoisting; correct the `(a{2,3})+` pin | §3.3 |
| **S3** | #1652 | Structural `==` (`__bynkEq` + dispatch), recursive equality-supporting check, one equality for stubs | §3.1 |
| **S4** | #1653 | Identifier hygiene at the `bynk_ts` boundary (`tag`/`kind` variant fields, host globals, runtime helpers, shadowed parameters and pattern bindings) | — |
| **S5** | #1654 | `is` in compound conditions. Root cause found in settling: the resolver walks `is`-bindings only in `if` arms of **fn** bodies and never walks handler bodies (`resolver.rs:1775-1796`). So `o is Some(n) && n > 0` is rejected in a `fn` and accepted in a handler. Unify the three walks: the checker publishes an `ExprId`-keyed binding table on `TypedCommons`, as `callees` already is | — |
| **S6** | #1655 | Workers target: `Option`/`Result` service params; test modules in workers output | — |
| **S7** | #1656 | Usefulness-based unreachable-arm detection | — |
| **S8** | #1657 | The safe-integer `Int` domain; strict `Int.parse`/`Float.parse`; `/0` traps; WebSocket open-param typing | §3.4 |
| **S9** | #1658 | `let x = <Effect>` without `<-` is an error. Corpus scan of 179 `let` sites in fixtures and examples: **zero** legitimate unbound-effect uses, so nothing breaks | — |
| **S10** | #1659 | Inference: `o == None`, annotated `Ok` in handlers, span-less suite diagnostic | — |
| **S11** | #1678 | Workers-target Durable Object calls bypass the boundary codec. The agent's `fetch` uses `request.json()` / `JSON.stringify(result)`, and `callDurableObjectMethod` uses `JSON.stringify({ args, deps })` / `response.json()` (`runtime.ts:1118-1130`), so `Bytes`, value-level `Map` and non-finite `Float` arguments and returns are mangled across intra-Worker agent calls | — |
| **S12** | #1679 | Unknown type names in service/agent handler signatures are not diagnosed (`on call(v: Bogus) -> Effect[Bogus]` checks with exit 0; `fn f() -> Bogus` is rejected), and the emitter writes a `/* unknown */` placeholder | — |
| **S13** | #1680 | `store Set[T]` has no keyability rule. `store Set[Pt]` checks, then fails `tsc` (TS2538); without `tsc`, every record keys as `"[object Object]"` | — |
| **S14** | #1685 | Store `Map`/`Set`/`Cache` keys collide with `Object.prototype`: a `__proto__` write is dropped and inherited names (`constructor`) read as present. Found in #1683's review | — |
| — | #291 | Platform capabilities under `bynkc test` | — |

**Gates**

| Slice | Issue | Content |
|---|---|---|
| **G0** | #1660 | §3.5: `expected_run.txt`, `behaviour_fixtures.rs`, cloning `InMemoryStorage`; triage 1402 and 385 |
| **G1** | #1661 | Doc gate compiles every `bynk` block |
| **G2** | #1662 | Diagnostic coverage and registry drift |
| **G3** | #1663 | Diagnostic recovery |

## 5. Front-loaded ADR candidates

Load-bearing and hard to reverse. Numbers are taken at merge, not here.

- **Structural equality** (S3, §3.1). Supersedes ADR 0142 D4's "whole-record `==` remains
  reference equality" scope note, and defines the equality-supporting rule recursively, as
  §2.3.5 already describes.
- **Agent state is stored in the wire shape** (S0, §3.2). Amends ADR 0124:
  - D4's merge mechanics become per-field;
  - corrects its "once per cold load" consequence;
  - records "no tolerant loader".
- **The `Int` domain** (S8, §3.4). The safe-integer domain; supersedes ADR 0319's open note
  and the `Int` half of ADR 0042 (`/0` traps); tightens ADR 0048's parse grammar.
- **Regex admissibility** (S2, §3.3). The ambiguity rule, its soundness obligations, and why
  first-character disjointness and runtime engines were rejected.

## 6. Threat model

**Assets:**
- A deployed Worker's CPU budget and availability.
- The integrity of string transformations over user data.
- Durable agent state and its atomic-commit guarantee.
- Whether a declared agent invariant is enforced at all.

**Adversary.** Any HTTP, queue or WebSocket caller who controls request input. The program's
author is trusted, but may be mistaken.

**Where verification happens:**
- **Regexes (§3.3).** Refinement regexes are author-written but run on attacker-controlled input
  at the boundary (`workers/<ctx>/index.ts` → `handlers.T.of(raw)`). Workers cannot interrupt a
  synchronous match, so the compile-time check is the only line of defence. R3 makes the
  guarantee exact for exponential blowup. For polynomial blowup it is exact when a length bound
  precedes the pattern.
- **`String.replace` (S1).** A user-controlled replacement is an injection shape today.
- **Agent state (§3.2).** Stored state is untrusted bytes by design (ADR 0124). S0 keeps the
  validation while fixing the shape, and makes the refused commit's writes invisible, restoring
  ADR 0109.
- **Equality (§3.1).** After S0 decodes state into fresh objects, an invariant written as
  `status == Paid implies …` has an antecedent that can never be true under reference
  equality. It is a safety check that silently does not run. Structural `==` makes the declared
  invariant the enforced one, which is why S3 lands with or before S0.
- **`Int` inputs (§3.4).** WebSocket open params currently carry an unvalidated string into an
  `Int`-typed agent key, splitting one logical agent across several Durable Objects (`?room=05`
  vs `Room(5)`). S8 closes the path.

## 7. Slice status

Live state is on the spine, [#1648](https://github.com/accuser/bynk/issues/1648), whose
sub-issue progress bar is authoritative.

## 8. Done when

- Every slice in §4 has landed or been closed by a recorded decision.
- The four ADRs in §5 are written.
- `behaviour_fixtures.rs` runs in CI under `BYNK_REQUIRE_TSC`, covering at least the §3.5
  priority set, with every slice above adding its runtime proof there.
- `InMemoryStorage` clones, so `bynkc test` observes storage the way workerd does.
- **On retire:** remove this doc, append the closing summary to
  `../archive/retired-tracks.md`, and close #1648.
