# The language as implemented — a review

**Reviewed at:** v0.290.1, 1 October 2026 (`e15e08da`)
**Scope:** whether the compiler does what the language says. The two previous reviews took the
pipeline's *architecture* ([30 August](2026-08-30-post-restructuring-review.md)) and the
*toolchain* around it ([3 September](2026-09-03-compiler-toolchain-review.md)). Neither ran Bynk
programs against the language's own rules. This one does: soundness probes against the spec,
diagnostic coverage, robustness under malformed input, feature completeness against the 1.0
definition, reproduction of the open defect issues, and a measured code-quality survey.
**Reference:** the book's normative spec (`site/.../book/spec/`), `design/bynk-type-system.md`,
`design/bynk-design-notes.md`, `design/bynk-1.0-definition.md`, and the ADRs they cite.

---

## How this review was produced

The tree was built, linted and tested first, on the pinned toolchain (`rustc 1.95.0`), with Node
24.15.0 and `tsc` 5.9.3 on `PATH`:

- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `RUSTDOCFLAGS=-D warnings cargo doc --workspace --no-deps` — all clean.
- `cargo test --workspace --no-fail-fast` — **1,908 passed, 0 failed, 0 ignored** across 175
  test binaries (4m02s).
- The external-tool gates were re-run with the skip-to-fail switches set
  (`BYNK_REQUIRE_TSC=1 BYNK_REQUIRE_WORKERD=1`, `tsc_verify`, `e2e`, the three workerd suites):
  all pass, nothing skipped. Node here is 24, so the two strip-types tests that the September
  review found always skip in CI **actually ran** — and passed.
- `tsc_verify` re-run with **TypeScript 7.0.2** first on `PATH`: 7 passed (Sep 3's experiment 2,
  repeated on today's corpus).
- `cargo xtask greenfield-status` — *table current*.
- The CI run on `main` for the reviewed commit (`e15e08da`, run 36823773123) is green, as are
  the two before it. The nightly `fuzz.yml` passed on each of the five nights before the review.

So, as before: **no finding below is a broken build, a failing test, or a tripped gate.** That
is the point of the review. The gates certify that emitted TypeScript *type-checks* and matches a
blessed golden. Nothing certifies that it *behaves* the way the spec says a Bynk program behaves,
and that is where most of the findings are.

Beyond the suite, the method was empirical throughout:

1. **~200 soundness probes.** Small programs, one or more per spec rule, hunting for programs
   wrongly accepted, programs wrongly rejected, and accepted programs whose output fails `tsc
   --strict` or misbehaves under `bynkc test`. Every accepted probe was type-checked under TS 5.9.3
   and 7.0.2. The two always agreed.
2. **Diagnostic coverage.** An instrumented copy of the workspace logged every `CompileError`
   built during `cargo test --workspace`, and the result was diffed against the 457-code
   `REGISTRY`.
3. **Robustness.** A seeded mutation sweep over the `.bynk` corpus through `bynkc check`,
   `bynkc compile` and `bynk fmt`, plus formatter idempotence and an LSP stdio drive. (See Part 5.)
4. **Completeness.** A feature-by-feature table against the 1.0 definition and the book, with a
   probe program per row, and every user-facing `bynk` block on the site classified.
5. **Every headline finding was reproduced a second time by hand** before it went in. Each
   carries its reproduction in the appendix.

---

## Headline

**The compiler is well built and its gates are honest about what they gate. What they gate is
"the output type-checks", not "the program does what Bynk says". In that gap there are two
defects that break real programs.**

**Finding 1: an agent that stores an enum or an `Option` breaks on its first write.**
`commitState` persists the agent's in-memory state as-is (`storage.put("state", s)`), so an enum
value is stored as `{ tag: "On" }`. `loadState` then validates the stored record with the HTTP
*wire* deserialiser (`bynk-emit/src/emitter/emit.rs:4913-4940`), which expects `{ kind: "On" }`.
So every load after the first commit throws `RehydrationViolation: A StructuralMismatch at s`.
After that, every handler on that agent key faults, permanently.

Reproduced under `bynkc test` for `Cell[enum]`, `Cell[Option[Int]]`, a `Cell` of a record
containing an `Option`, and `Map[String, enum]`. The faulty pair is in the production output,
not only the test harness: the workers target's `handlers.ts` emits it (checked for
`Cell[enum]`). Records of scalars are fine, which is why `examples/orders` works.

The goldens of fixtures `139_agent_state_zero_option` and `155_state_sum_machine` contain the
faulty pair. They have done so since the gate landed in `5c957082` (ADR 0124, 26 June). Neither
fixture's test reloads a key after writing it. #539 calls durable-state evolution "the gap that
historically kills durable-state platforms", and this defect sits squarely on that surface.

**Finding 2: `==` compares references for records, sums with payloads, `List` and `Option`.**
`Some(1) == Some(1)` is `false`. `Some(1) != Some(1)` is `true`. `P { x: 1, y: 2 } == P { x: 1, y:
2 }` is `false`, and the test runner prints `actual: {"x":1,"y":2} == {"x":1,"y":2}` as a failure.
`lower.rs:4812-4834` special-cases `Bytes` (ADR 0142 D4) and lowers every other type to host
`===`.

This is *known*: ADR 0142:54 and :127 say whole-record `==` "remains reference equality today;
changing that is a separate decision". But the separate decision was never taken or filed. The
Settled type-system spec says the opposite (§2.3.5, *"two values of the same sum type are equal
iff they have the same variant tag and their corresponding fields compare equal"*). The checker
accepts `==` on these types without a word, and the book's static semantics (§5.2) requires only
"same type". It also compounds Finding 1: if rehydration were fixed, `__state.s === St.On` would
still be `false` after storage round-trips the object.

**Finding 3: the CI gates cover the corpus well, and the corpus has gaps.**
- 20 confirmed soundness defects, every one reproduced by a probe of one to ten lines.
- 99 of 457 diagnostic codes (21.7%) are never produced by any test. 26 of a 29-code sample fire
  correctly from a one-line program.
- The documentation gate compiles 93 of the site's 291 `bynk` blocks.
- `tsc_verify` covers 99 workers-target fixtures, and none of them has an `Option` service
  parameter (which fails `tsc` on that target).

Two of the soundness defects are security-relevant (§1.3): an ambiguous-alternation regex in a
refinement turns a 28-character URL path into **25 seconds** of Worker CPU, and `String.replace`
expands `$&`/`$1` in its replacement.

None of this is a structural problem. Each defect is local, and most are one-line fixes. They
are the residue of a project that spent August and September on the compiler's internals and its
gates, which are now very good, while the language's *runtime semantics* went unprobed.
Since the September review the tree has had **36 non-stamp commits**: 30 dependency bumps, the
September review itself, three changelog and stamp housekeeping commits, one advisory fix, and one
README refresh. There were no commits at all from 5 to 29 September. That sets the context for
Part 0.

---

## Part 0 — What the 3 September review asked for, four weeks later

| # | Ask | Status at `e15e08da` |
|---|---|---|
| 1 | Run the strip-types gate for real (Node 22 on the `test` legs, `BYNK_REQUIRE_STRIP`) | **Not done.** `ci.yml:289` and `release.yml:150` still pin Node `"20"`; there is no third `BYNK_REQUIRE_*`. (Both tests ran and passed on this host's Node 24, so the invariant holds today. It is still unchecked in CI.) |
| 2 | Verify emitted output under TypeScript 7 | **Not done** in CI; `typescript@5` at `ci.yml:294`, `release.yml:153`, `tsc_verify.rs:58`, `test_runner.rs:372`. Re-measured by hand: the corpus is clean under 7.0.2. |
| 3 | Make the extension's server pin true by construction | **Recurred as predicted.** v0.290.0 shipped on 4 September, closing the 35-day drought. This morning's stamp (`e15e08da`) bumped the workspace to 0.290.1, and `vscode-bynk/package.json:13` now pins `v0.290.1`, but `gh release view v0.290.1` says *release not found*. That is the exact failure mode Sep 3 §3.1 said a habit cannot prevent. |
| 4 | Raise `NODE_MAJOR_FLOOR` to 22 | **Not done** — `bynk-emit/src/lib.rs:94` is still 18. |
| 5 | Let `bynk test` read the compiler skew | **Not done** — `bynk/src/test.rs` has no `skew` reference outside its module comment. |
| 6 | One harness for both test gates | **Not done** — `release.yml:178` is still `cargo test`. |
| 7 | `wasm-bindgen-cli` pin drift test; `COMPATIBILITY_DATE` policy | **Not done** — `ci.yml:502` and `deploy-playground.yml:193` pin `0.2.126`; `Cargo.lock` is `0.2.127`; `wrangler.rs:13` still `2024-11-01`. |
| 8 | Repoint the dead track citations | **Not done** — **96** references to seven deleted `design/tracks/*.md` files (`the-ir.md` 42, `semantics-in-the-checker.md` 35, …); the code-quality survey also found 5 of 9 sampled `file.rs:N` comment citations point at the wrong line. |

None of the eight landed. That is not a criticism of the asks' priority, since the tree was idle,
but it means every September finding still stands, and item 3 has now demonstrated itself.

---

## Part 1 — Programs the compiler accepts and gets wrong

Of the 20 confirmed findings, Findings 1 and 2 are in the headline. The rest group into three
classes.

### 1.1 Accepted, and wrong at runtime

| # | Defect | Minimal program | Observed | Spec |
|---|---|---|---|---|
| 1.1a | `String.replace` is `replaceAll(a, b)` with a string `b`, so JS expands `$&`, `$1`, `` $` ``, `$'`, `$$` | `"aaa".replace("a", "$&b")` | `"ababab"`; expected `"$&b$&b$&b"` | static-semantics §5 (String surface); ADR 0046. Emission at `lower.rs:3850`; fix is `replaceAll(a, () => b)` |
| 1.1b | Exhaustiveness does not report an arm made unreachable by an *earlier, more general* nested pattern | `match o { Some(_) => 1; Some(Red) => 2; None => 3 }` | accepted; the emitted narrowing then fails `tsc` (TS2367) | static-semantics:608, arms "MUST NOT be unreachable" |
| 1.1c | Redundant trailing arm after full coverage is silently accepted | `match b { true => 1; false => 2; _ => 3 }` | exit 0 | same; unreachable arms are only detected after a *leading* wildcard |

Plausible but spec-ambiguous (reproduced; filed here for a decision, not as defects):
- An effectful call that is not bound with `<-` (`let e = Counter("k").bump()`) still runs, eagerly
  and unawaited, and its write lands with no warning. static-semantics:568-571 admits the
  eager-`Promise` translation makes this observable. It also bypasses the `do`/`~>` gates.
- `Int.parse` uses JS `Number()`: `"0x10"` → `Some(16)`, `"1e3"` → `Some(1000)`, `" 7 "` →
  `Some(7)`, against static-semantics:129-133's "full-string; leading/trailing garbage is `None`".
- `Int` range: the lexer takes any `i64`, the JSON deserialiser takes any `Number.isInteger`
  (incl. `1e300`), `Int.parse` takes only `isSafeInteger`. `9007199254740993 ==
  9007199254740992` is `true`. ADR 0319 acknowledges the i64-vs-2⁵³ gap; it is not yet a decision.

### 1.2 Accepted, and the output fails `tsc --strict`

Each of these passes `bynkc check` and `bynkc compile` with exit 0 and then fails the compiler's
own promise that output is `tsc --strict`-clean (compilation-model.md:91,98).

| # | Defect | Trigger | tsc |
|---|---|---|---|
| 1.2a | A variant payload field named `tag` collides with the emitted discriminant | `type S = \| A(tag: String) \| B(n: Int)` | TS2300/TS1117 |
| 1.2b | A payload field named `kind` collides with the JSON wire discriminant | same, with `kind`, plus `Json.encode` | TS1117 |
| 1.2c | User names that shadow JS globals or emitted runtime names are not mangled | `type Math`, `type Number`, `type JSON`, `type Error`, `type Promise`, `type Uint8Array`, `type JsonValue`, `type BoundaryError`, `fn serialise_P` | TS2339, TS2351, TS2315, TS2440, TS2393 |
| 1.2d | A pattern binding with the same name as its scrutinee | `if o is Some(o) { o }` | `const o = o.value` — TS2448 |
| 1.2e | `let` re-binding a function **parameter** (let/match/lambda re-binding all work) | `fn t(x: Int) -> Int { let x = 5 ⏎ x }` | TS2300 |
| 1.2f | Two refined `is` tests joined by `&&`: the second receiver temp escapes its IIFE | `if n is Q && m is Q { dbl(m) }` | TS2304 `__r1` |
| 1.2g | A function type as a `List` element is printed without parentheses | `List[(Int) -> Int]` | TS1005 |
| 1.2h | **Workers target only:** a service parameter of type `Option`/`Result` is used in `compose.ts` without being imported | `on call(o: Option[Int])` | TS2749 (bundle target is clean) |
| 1.2i | **Workers target only:** a project with a context-targeting `suite` emits `tests/*.test.ts` importing `./../<ctx>.js`, which the workers layout does not have | `examples/orders`, `examples/todo` | TS2307 |

Rows 1.2a–1.2g share a root cause: **the emitter has no identifier-hygiene pass.** Bynk names
pass through to TypeScript verbatim, next to discriminants (`tag`, `kind`), runtime helpers
(`serialise_*`, `__r1`, `JsonValue`) and host globals. static-semantics:35-40 promises that a
re-binding "gets its own emitted identifier", and that promise is kept for `let` but not for
parameters or pattern bindings. A single renaming step at the `bynk_ts` boundary, or reserving
these names in the checker, would close the whole class.

Rows 1.2h and 1.2i are a coverage gap. The examples gate (`bynkc/tests/examples.rs:118-125`)
compiles `src/` only and says test modules are "`bynkc test`'s concern". 99 of 423 positive
fixtures are workers-target, and none has an `Option` service parameter.

### 1.3 Two that matter for security

- **ReDoS through an accepted refinement.** The `catastrophic_regex` guard
  (`bynk-check/src/checker/refinements.rs:895-903`) catches nested quantifiers only. The code
  states, and a test pins (`:1123-1133`), that ambiguous alternation (`(a|a)+`, `(\d|\d\d)+`,
  `(foo|foobar)+`) is exponential and *not* caught, and calls it "a deferred follow-up (#724)".
  **#724 is closed**, and no open issue tracks the remainder. End to end:
  `type Slug = String where Matches("(a|a)+")` on a `GET("/s/:slug")` path parameter checks and
  compiles for workers. The generated route entry (`workers/web/index.ts:28`) calls
  `handlers.Slug.of(__raw_slug)` on the raw path segment, which runs
  `new RegExp("^(?:(a|a)+)$")`.
  Timed under Node 24: 20 chars 98 ms, 24 chars 1.6 s, 26 chars 6.4 s, **28 chars 25.6 s**. The
  author writes the regex, but the attacker supplies the input. static-semantics:333-339 gives
  ReDoS protection as the rule's reason, so users will reasonably believe it is covered.
- **`String.replace` (1.1a)** is a classic injection shape when the replacement is user data.

### 1.4 Rejected, and should not be

- `o is Some(v) && v > 0`: `is` bindings do not reach the right operand of `&&` or `implies`
  (`unknown name 'v'`). They do reach the `then` block. The Settled spec's own example
  (type-system §2.3.6) uses exactly this shape.
- `o == None` cannot infer `None`'s type from the other operand
  (`cannot_infer_option_type_param`). Inside a `suite` the diagnostic carries **no file, line or
  span**.
- Else-branch narrowing after negation (`if !(o is Some(v)) {…} else { v }`) is Settled in
  type-system §2.3.6 and rejected.
- In an HTTP handler, `let r: Result[Int, String] = Ok(1)` is rejected as
  `ambiguous_constructor`. The annotation is ignored in favour of the handler's `HttpResult`
  return type (`checker/expressions.rs` ~1656).

### 1.5 What was sound

This list matters as much as the defects, because it is most of the language:
- Refined-literal admission through every position probed: branches, arms, lists, record
  fields, lambdas, `:=`, store initialisers.
- Arithmetic widening on refined values.
- All the refinement-declaration checks.
- Narrowing scope (then-only, no leak, `||` does not narrow).
- Exhaustiveness for nested `Option[Result]`, guards, or-patterns and literal arms.
- Variance (contravariant parameters, covariant `List`/`Option`/`Result`, invariant `Map` keys).
- Opaque-type encapsulation.
- Every effect-discipline rejection.
- Storage typing.
- Recursive types through JSON.
- Shadowing rules.
- String escapes, interpolation and UTF-16/code-point semantics.
- JSON codec round-trips, including `Option[Option[Int]]`.
- Record fields named `constructor`, `then`, `toString` and `hasOwnProperty`.

The type checker is in good shape. The defects are almost all at the *emission* boundary.

---

## Part 2 — What users can test

The testing story is where Finding 1 hid, so it is worth stating what a user can and cannot
exercise today.

**#291 is half fixed and should be retitled, not closed.** Its literal symptom (`makeSurface`
missing, TS2339) is gone: a context that `consumes bynk { Logger }` now compiles and its pure
helpers test fine. But:
- A unit that *uses* a platform capability crashes at runtime under `bynkc test`. A service with
  `given Logger` gives `TypeError: Cannot read properties of undefined (reading 'info')`. The
  generated surface is `{ Logger: undefined as unknown as bynk.Logger }` (`out/tests/svc.test.ts:31`):
  the cast satisfies `tsc` and the value is `undefined`.
- The documented `stub` on a platform capability (`stub Logger.info(_) returns ()`; the testing
  guide's own example is `stub Kv.get(_) fails`) emits a partial stub object that fails `tsc`
  (TS2345, `'error' is missing in type '__Stub_Logger'`). The same pattern on a *user* capability
  passes.
- `expect Logger.info called once` is rejected as `observe.not_a_seam`.

The consequence shows in the examples. Three of eleven (`event-log`, `sessions`,
`webhook-relay`) have no tests, and their READMEs cite #291. The other eight test mostly pure
`commons` logic. A user cannot write a test that exercises an agent or service using `Logger`,
`Kv` or any other platform capability. Finding 1 is exactly the kind of bug such a test would
have caught.

**Every example otherwise holds up.** All 11 check, compile on both targets, and pass `tsc
--strict` on the bundle target. The 8 with tests pass (29 cases). The two workers-target failures
are 1.2i.

---

## Part 3 — Completeness against the spec and 1.0

### 3.1 The 1.0 gates

`design/bynk-1.0-definition.md` defines 1.0 as three gates:

| Gate | State |
|---|---|
| 1 — Foundations stability | **Implemented, documented and tested, with one naming mismatch.** The definition (`:55`) and design-notes §2 freeze "cross-agent calls via `Ref[A]`"; `Ref` is an unknown type to the compiler, and appears on zero site pages. The shipped, documented, tested form is `Agent(key).handler()`. Amend the definition, or build `Ref`. |
| 2 — Deploy | **Met** (`bynk deploy`; track retired v0.220.2). |
| 3 — Durable-state migrations (#539, P0) | **Not started, and no track exists.** No schema fingerprint, no `migrate`, no rename detection, no enumeration verb (`bynk migrate` → unrecognised subcommand). ADR 0124 D3/D5 deliberately deferred all of it. Probed: renaming `store owner` → `store ownerName` checks and compiles silently. `ownerName` reads as `""`, and the old `owner` key is carried along as an orphan and written back on every commit. Renaming the agent class re-emits `[[migrations]] tag = "v1" new_classes = ["Basket"]` with the same tag. That Cloudflare would orphan the old class's data is inferred, not deployed. |

Gate 3 is the 1.0 blocker. Finding 1 belongs to the same surface (state that persists across
deploys and loads). It should be fixed before the migration track starts, because that track will
build on `loadState`/`commitState`.

### 3.2 The open tracks

| Issue | State |
|---|---|
| #936 Events | Done except slice 4b; every §8 item ticked (slices 0–4, v0.238–v0.244). The issue body and the track header (*Settling (draft)*) are stale. |
| #990 Events 4b (`via schema` ranges) | Not started. `SchemaVersionPattern::Literal` only (`ast.rs:1725`); `2..` is a clean parse error; docs say "future". `design/tracks/events.md:442,644,732` still calls it "unfiled". |
| #921 Idempotency | Slice 0 + key scoping shipped (ADRs 0281–0283); the only open slice was decided as convention by ADR 0294. **Ready to retire.** The in-isolate `Map` provider does not dedup across isolates; documented (`bynk-capabilities.md:67`). |
| #554 (P1) | 1 of 3: idempotency shipped. ADR 0020 still *Open* (waits on packaging). The composition-root `tsc` hole no longer reproduces: callers now forward whole-context `deps`. The encapsulation question itself is still open. |
| #843 Packaging, #918 Envelope refinement, #551 N:M grouping, #555 Observability, #856 OpenAPI | Not started. #856's dependency (#855) is closed, so it is unblocked. `design/tracks/packaging.md`, which #843 calls "a committed Draft", does not exist. |

No track is half-shipped in a user-visible way. Everything unbuilt fails loudly or is documented
as future, with one exception: **`bynk.toml` silently accepts unknown tables.**
`[dependencies]`, `[deploy] groups = …` and `[workspace]` all check and compile with exit 0, while
a typo *inside* `[paths]` gets a "did you mean". A user writing the packaging or N:M syntax they
read about in a design doc gets silence.

### 3.3 Documented to users, rejected by the compiler

The doc gate (`bynkc/tests/doc_examples.rs:9-16`) compiles only blocks that begin with `commons`
or `context`: **93 of 291** site blocks. Of the 192 it does not see:
- `store level: Cell[Int where Positive]` (inline refinement in a store type) is a parse error. It
  appears in `troubleshooting/agents-non-zeroable-state-field.md:18-22` and :31 (both the
  trigger *and the fix*), `guides/agents-and-state/stateful-agent.md:95` and
  `tutorials/05-stateful-agent.md:65`. The troubleshooting page cannot produce the error it
  explains.
- `"sent: " ++ tracking` (`docs/emission.md:154`). There is no `++`, and `+` on `String` is a
  type error.
- Seven diagnostic codes cited in prose that do not exist, e.g. `bynk.parse.cron_in_agent`,
  `queue_in_agent` and `http_in_agent` (all three are `handler_in_agent`), and a whole
  troubleshooting section for `bynk.queue.return_not_effect_result`.

The other direction: the normative spec (`spec/index.md:5`, "defines the language as compiled at
v0.290") has no `event_decl` or `messages_decl`. `spec/appendix-planned.md:11-16` and
`spec/scope.md:19-20` call Events and storage kinds unshipped. `design/bynk-status-and-roadmap.md`
§4/§7 lists query algebra, rich storage and held connections as deferred, though all three are
retired as shipped. `design/bynk-design-notes.md:1992` says the compiler "is written in Go". The
root `CHANGELOG.md` stops at v0.142.0; the live changelog is the site's.

### 3.4 The two recorded spec contradictions

- **#1530 tuples:** the implementation follows **ADR 0120** (no tuples): `(1, "a")`, `(Int,
  String)` and `let (a, b)` are all rejected, and the user docs agree. The stale side is
  type-system §2.7.6 and design-notes §11.
- **#1529 effect inference:** for named declarations the implementation follows **design-notes
  §15** (declared, not inferred). `fn` requires `->`, `given` is undeclared-checked, and a free
  `fn` cannot use a capability. One carve-out: an unannotated *lambda* containing `<-` gets
  `Effect` inferred. So §2.8.4's "inferable on internal helpers" holds for lambdas only, and
  design-notes §15's "complete inference for unannotated code" holds for nothing.

Both issues can be resolved by editing the stale document; neither needs a design decision.

---

## Part 4 — Diagnostics

Registry: **457 codes** (`bynk-syntax/src/diagnostics.rs` `REGISTRY`), 381 built in
`bynk-check`, 63 in `bynk-syntax`, 5 in `bynk-project`. `bynk-emit`, `bynk-driver` and `bynkc`
build none, which is the R9 architecture holding.

| Measure | Codes | % |
|---|---|---|
| Produced at least once by `cargo test --workspace` | 358 | 78.3 |
| …excluding codes reached only by `fuzz_smoke` (random input, asserts nothing) | 341 | 74.6 |
| Asserted by a test (negative-fixture expectation, or code string in test code) | 332 | 72.6 |
| **Never produced by any test** | **99** | 21.7 |

Weakest families: `bynk.agent.*` (**6 of 6** untested), `types` 19/78, `resolve` 13/36, `parse`
10/48. Of 29 sampled untested codes, **26 fire correctly from a one-line program** and can become
negative fixtures as-is. The rest:
- **Dead:** `consumes.in_commons` (`project_model.rs:1116`). The parser rejects the shape first.
- **Shadowed:** `types.unknown_static_member` and `types.opaque_record_construction` duplicate
  `resolve.*` checks that always fire first, with identical messages. So, very likely, do the
  `symbols.rs` halves of `agent`/`provider`/`service`/`actor.outside_context`.

**Registry drift (#1531's trigger, now observed).** `bynk explain bynk.fmt.roundtrip` answers "not
a diagnostic code the compiler emits", yet `bynk-fmt/src/fmt.rs:239` emits it. The same is true of
`fmt.comment_loss`, `wasm.panic`, `wasm.strip_failed` and `deploy.contract_skew`.
`bynkc/tests/diagnostics_registry.rs` scans five crates and misses `bynk-fmt`, `bynk-wasm`,
`bynk`, `bynk-driver`, `bynk-ide` and `bynk-lsp`. #1531 is filed as "Deferred, no trigger". This
is the trigger.

**Recovery.** The CLI stops at the **first** syntax error in a file (always exactly one, never a
cascade, and later errors are lost); it does continue across files. The editor path recovers by
skipping the broken declaration, which then cascades: one missing comma in `49_money` produces 21
diagnostics, 20 of them `unknown_type` echoes. In both paths, **any resolve error in a unit
suppresses every type error in that unit**, even in unrelated functions: `fn a() -> Int { "s" }`
is reported only once `fn b() -> Int { nope }` is fixed. Stale messages: the `-> Result[Int]` note
still says "v0.1 has no other generic types", and the unknown-type note omits `Float`, `Duration`,
`Instant`, `Bytes`, `List` and `Map` from what is in scope.

---

## Part 5 — Robustness and the formatter

**The front end does not crash.** The corpus was 1,586 inputs: 1,292 tracked `.bynk` files plus
294 `bynk` blocks extracted from the site. Four seeded mutation rounds over it produced **91,736
mutants**, run through `bynkc check`, `bynk fmt -`, `bynkc compile` (subset) and a stable-Rust
mirror of the two `fuzz/` targets' invariants (no panic; every span in bounds and on a char
boundary). That is about 293k invocations.

The rounds covered:
- line and byte truncation, line delete/duplicate/swap, and 16 insertion classes (unbalanced
  brackets and quotes, `/*`, NUL, BOM, ZWJ, CR/CRLF, invalid UTF-8);
- nesting up to 20k deep, 1M-character tokens and 1M blank lines;
- token-level edits and cross-file splices;
- semantic edits on the 385 clean files;
- 597 project-form fixtures, plus broken `bynk.toml`s.

Result: **no panics, no stack overflows, no aborts, no hangs.** In the semantic round 27.5% of
mutants checked clean and went through emission, so this exercised the back end too, not only
the parser. Deep nesting is capped at 64 (`MAX_NESTING_DEPTH`) with a proper diagnostic, from
both the lexer and the parser. This is the best-tested surface in the tree. The nightly fuzzing
and the depth cap are doing their job.

**The language server does not crash either.** The LSP drive sent about 225k requests over 803
inputs: completion, hover and signature help after each of 29,818 single-character keystrokes,
plus semantic tokens, symbols and inlay hints, and 306 mutants opened inside real project roots.
No panics, no request timeouts, and every server shut down with exit 0. (Part 6 notes that a
panic, were there one, would be swallowed silently.)

**The formatter is idempotent and semantics-preserving, with one exception.** Across 1,388
formattable inputs and five styles (default, widths 40 and 20, two indent and trailing-comma
variants):
- idempotent 1,388/1,388;
- `--check` agrees with a byte comparison 1,388/1,388;
- no `--` comment lost;
- identical diagnostics after formatting 1,386/1,388;
- identical emitted TypeScript 383/383 for single files and 25/25 for projects (modulo source
  maps);
- AST differences are all benign (an explicit trailing `()` becoming implicit).

The exception, and the two misses above: **the formatter silently deletes an orphaned doc
block.**
`printf 'commons d\n\n---\nx\n---\n\nfn f() -> Int { 1 }\n' | bynkc fmt -` prints the file without
the `---` block, exit 0, nothing on stderr. The same happens between declarations and at the end
of the file. `bynkc check` warns `parse.orphan_doc_block` on the same input, but the formatter's
comment-loss guard (`bynk.fmt.comment_loss`) counts only `--` comments. In-place `bynk fmt` exits
0, and `--check` then passes, so no CI gate sees the loss. Two corpus files are affected
(`fixtures/behaviour/warning_severity_single.bynk`,
`fixtures/positive/308_orphan_doc_block_warns/`). It is user-visible data loss in a tool users run
on save.

**Two smaller findings:**
- **Rich diagnostic rendering is linear in line length at about 15 µs per column, with a
  separate ANSI escape per character.** A 1M-character line takes 13 s and 28.5 MB of stderr
  under `bynkc check` and under `bynk fmt` when it reports an error; `--format short` takes 0.1 s
  and 127 bytes. Colour is emitted even when stderr is not a TTY, and `NO_COLOR=1` is ignored.
  Both are cheap to fix, and both matter to anyone piping `bynk` into CI logs.
- **One malformed JSON-RPC message ends the language server with exit 0.** A `didOpen` whose text
  carries a lone-surrogate escape (`\udcff`, which `JSON.stringify` produces for a lone JS
  surrogate), or a truncated body, does it. The only trace is a line in `~/.bynk-lsp.log` from
  `tower_lsp::transport`. This is upstream behaviour, and another reason to weigh the maintained
  fork.

---

## Part 6 — Code quality, measured

Non-test code only; the method (a masking lexer, brace-matched function extents) is in the appendix.

**Size and shape.** ~170k lines of Rust across 18 crates. `bynk-check` (28.9k code lines) and
`bynk-emit` (24.5k) are two-thirds of the compiler proper. 16 files exceed 3,000 lines. The
longest functions:

| Lines | Function | Location |
|---:|---|---|
| 2,064 | `emit_agent` | `bynk-emit/src/emitter/emit.rs:4255` |
| 1,118 | `lower_method_call` | `bynk-emit/src/emitter/lower.rs:1444` |
| 895 | `emit_worker_entry` | `bynk-emit/src/project/workers_entry.rs:296` |
| 663 | `check_method_call` | `bynk-check/src/checker/calls.rs:2095` |
| 616 | `emit_worker_compose` | `bynk-emit/src/project/workers.rs:264` |

33 functions exceed 300 lines, 75 exceed 200, and `#[allow(clippy::too_many_arguments)]` appears
70 times (64 in check + emit). `emit_agent`'s own doc comment calls much of it "genuinely raw
hand-templated text … zero bynk_ts nodes". Finding 1 lives in it (the `format!`-built rehydration
lines at `:4929-4991`), as do 1.2a/1.2c, and that is not a coincidence. Hand-templated text is
where identifier hygiene and shape agreement between writer and reader go unchecked.

**What is good, measurably:**
- **No `unsafe` and no `todo!`/`unimplemented!`** in any crate.
- Panics are disciplined. Nearly every `panic!`/`unreachable!`/`expect` in check/emit/lower
  carries a "bynk internal error (finding/ADR …)" message. Emission requires a `certify`-produced
  `CheckedProgram`. Every sampled site was a justified invariant, not a crash reachable from user
  input.
- Doc coverage on `pub` items is 88–100% in the compiler crates.
- The dependency graph is lean: core crates have 1–3 external dependencies, `tokio` is confined
  to the LSP, and `oxc` to `bynkc`/`bynk-wasm`.

**What is not:**
- **Wildcard arms are unenforced.** `wildcard_arms` reads 310, and about 81% of those (heuristic)
  are on compiler-owned enums: `Ty` 61, `ExprKind` 51, `CommonsItem` 39, `TypeRef` 32. The
  `wildcard_enum_match_arm = "warn"` table in `Cargo.toml` binds nothing, because no crate opts in
  with `[lints] workspace = true`. The comment explaining that cites a deleted track.
- **Hand-kept couplings.** `keep_in_sync` reads 201; sampled, about half are real. One of them is
  the `is`-binding flow walk, written three times: `resolver.rs:2117`,
  `checker/expressions.rs:3852` and the emitter's `gather_is_bindings_for_emit` (`lower.rs`
  ~2930). Each copy is a place where the checker's and the emitter's view of a binding's scope
  can drift. Finding 1.2f is the emitter's half of this surface: the right operand of `&&` is
  wrapped in an IIFE so that the left operand's narrowing is in scope, and the same IIFE scopes
  the right operand's own receiver temp (`__r1`) away from the `then` block that reads it.
- **The test pyramid is inverted in the two big crates.** Tests per 1k code lines: `bynk-check`
  5.8, `bynk-emit` 8.0 (vs 33–61 in `bynk-ts`, `bynk-lower`, `bynk-project`, `bynk-lsp`,
  `bynk-ide`). Their coverage is almost entirely end-to-end through 1,166 fixtures, which is
  powerful, but the fixtures assert *golden text* and `tsc`-cleanness, so they bless whatever was
  emitted (Finding 1). `bynk-ir` has no tests.
- **The language server swallows analysis panics.** `bynk-lsp/src/lib.rs:631-637` runs diagnosis
  in `spawn_blocking` and treats a `JoinError` as `else { return; }`. A panic leaves stale
  diagnostics with no log line. `tower-lsp` 0.20 is unmaintained (fork: `tower-lsp-server`).
- **`bynk-check` exports too much.** All 25 modules are `pub mod`, and at least 114 of 476 `pub`
  items (24%) are never named outside the crate, which hides them from `dead_code`.
- One silent miscompile path: `lower.rs:2976` writes `"(/* TODO: complex is-receiver */ )"` into
  output where its sibling arms panic. It is the only non-test `TODO` in the workspace.

---

## Part 7 — Things worth recording as correct

- **The gates are honest.** Every external-tool gate passes when forced not to skip. The
  `tsc_verify` corpus is clean under TS 5 *and* 7, nightly fuzzing is green, and the greenfield
  table is current. Every defect above is *outside* what a gate claims to certify, not a gate
  lying.
- **The checker is sound on the core.** Part 1.5 is long. Refinements, narrowing scope,
  exhaustiveness over nested sums, variance, opaque encapsulation and effect discipline all held
  under adversarial probing.
- **Nothing crashes.** About 92k mutants and about 225k LSP requests produced zero panics or hangs.
  The formatter is idempotent and output-preserving across five styles. The depth cap, the fuzz
  targets and the "bynk internal error" discipline are visibly working.
- **Unbuilt features fail loudly.** Every unimplemented track probed gave a parse error or an
  "unrecognised subcommand", not a half-working surface. The `bynk.toml` table check is the one
  exception.
- **Diagnostics are architecturally clean.** There is one registry, the emitter builds none, and
  project-level diagnostics are the only ones with `Span::default()` (all defensible).
- **The error messages that exist are good.** The probes repeatedly hit precise, actionable
  messages: `bind_in_pure_context`, `undeclared_capability`, the `[paths]` did-you-mean,
  `nesting_too_deep`.

---

## Part 8 — What to do

Ordered by value over cost.

1. **Fix agent-state persistence (Finding 1) and add the test that would have caught it.**
   Either serialise on commit (`storage.put("state", serialise(s))`, then deserialise *into*
   the state on load), or validate with an in-memory-shape validator. Add a fixture that writes an
   enum, an `Option` and a `Map[_, enum]`, *reloads the same key*, and reads them back, on both
   targets. Then audit the blessed goldens of 139 and 155. Do this before the Gate 3 migration
   track builds on `loadState`.
2. **Decide `==` (Finding 2) and record the decision.** Either derive structural equality
   (Settled type-system §2.3.5 already specifies it), or reject `==` on non-primitive types in the
   checker with a pointer to a comparator. Today's middle ground, accepted and silently wrong,
   is the one option no document endorses. File it; ADR 0142 already calls it "a separate
   decision".
3. **Close the two security-shaped defects.** `replaceAll(a, () => b)` (one line). For ReDoS,
   either extend the guard to branch overlap, or reject alternation under an unbounded quantifier
   unless the branches are provably disjoint (conservative, like the existing check). Either way,
   file an open issue: the code's "deferred (#724)" points at a closed one.
4. **Stop the formatter deleting orphaned doc blocks.** Count `---` blocks in the comment-loss
   guard (a one-line widening), so the failure is a refusal rather than silent deletion. Then
   decide whether to preserve orphans or attach them.
5. **An identifier-hygiene pass at the `bynk_ts` boundary.** One mangling step for user names
   that collide with `tag`/`kind`/runtime helpers/host globals, and fresh names for shadowing
   parameters and pattern bindings. This closes rows 1.2a–1.2e as a class.
6. **Widen what the gates see, cheaply.**
   - (a) Compile *all* doc blocks, treating fragments as `,ignore` only when marked. That turns
     §3.3 into failing tests.
   - (b) Add the 26 reachable-but-untested diagnostic probes as negative fixtures. Delete
     `consumes.in_commons` and the shadowed duplicates.
   - (c) Extend `diagnostics_registry.rs` to every crate, which closes #1531's trigger.
   - (d) Compile the examples' split form to workers in the examples gate, and add an `Option`
     service parameter to a workers fixture.
   - (e) Add a behavioural fixture class that asserts runtime results, not just golden text.
7. **Retitle #291** and make platform capabilities stubbable and observable in tests. That
   unblocks testing handlers at all, and three examples.
8. **Reject unknown `bynk.toml` tables**, with the same did-you-mean `[paths]` already has.
9. **Documentation truth pass.** Fix the store-refinement and `++` examples and the seven phantom
   codes. Resolve #1529/#1530 by editing the stale sides. Bring `spec/` up to Events and messages.
   Fix the stale status-and-roadmap, design-notes ("Go") and root `CHANGELOG.md`. Amend the 1.0
   definition's `Ref[A]` (or build it). Retire #921. Refresh #936's body and the events track
   header.
10. **The September list**, still all open. Item 3 (the extension pin) is now demonstrated rather
   than predicted.
11. **Then Gate 3.** Open the migration track. Its rename probe in §3.1 is a ready-made first
    fixture.

Items 1–5 are a few days' work, and none needs a new ADR except item 2's decision. They would
move the tree from "every output type-checks" to "every output does what the spec says" on
everything this review probed.

---

## Appendix — how to reproduce every number

Run from the repository root at `e15e08da`. `B=target/debug/bynkc` after `cargo build -p bynkc -p bynk`.

**Baseline.**

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps
cargo test --workspace --no-fail-fast 2>&1 | tee test.log
grep -E '^test result' test.log | awk '{p+=$4; f+=$6} END {print NR, p, f}'   # 175 1908 0
BYNK_REQUIRE_TSC=1 BYNK_REQUIRE_WORKERD=1 cargo test --locked -p bynkc \
  --test tsc_verify --test e2e --test events_boundary_workerd \
  --test events_ordering_workerd --test workers_runtime_smoke
npm install -g --prefix /tmp/ts7 typescript@7
PATH=/tmp/ts7/bin:$PATH BYNK_REQUIRE_TSC=1 cargo test --locked -p bynkc --test tsc_verify
cargo run -q -p xtask -- greenfield-status
git log --oneline 6370cfc..HEAD | grep -vc 'chore(stamp)'                      # 36
```

**Finding 1 — enum state.** `src/m.bynk`:

```bynk
context m
type St = enum { On, Off }
agent A {
  key id: String
  store s: Cell[St] = Off
  on call turnOn() -> Effect[()] { s := On }
  on call isOn() -> Effect[Bool] { s == On }
}
```

`tests/m.bynk`: `suite m { case "reload" { do A("k").turnOn() ⏎ let v <- A("k").isOn() ⏎ expect v } }`.
`bynkc test .` → `RehydrationViolation: A StructuralMismatch at s`.
`bynkc compile . --target workers -o ow` and read `ow/workers/m/handlers.ts`: `St.On` is
`{ tag: "On" }`, `commitState` is `storage.put("state", s)`, and `deserialise_St` switches on
`obj["kind"]`. Swap in `Cell[Option[Int]]` or `Map[String, St]` for the other two shapes.

**Finding 2 — equality.**
`fn eqOI(a: Option[Int], b: Option[Int]) -> Bool { a == b }` with
`expect eqOI(Some(1), Some(1))` fails. Records, payload sums and `List` behave the same way.
`sed -n 4812,4834p bynk-emit/src/emitter/lower.rs`.

**1.1a replace.** `"aaa".replace("a", "$&b")` → `"ababab"`. `sed -n 3850p bynk-emit/src/emitter/lower.rs`.

**1.2 accept-then-tsc.** For each trigger in the table: `bynkc check .` (exit 0), then
`bynkc compile . -o o && tsc -p o --noEmit`. For 1.2h add `--target workers` and compare with the
bundle target. For 1.2i: copy `examples/orders`, `bynkc compile . --target workers -o out-w`,
then `tsc -p out-w/tsconfig.json --noEmit` → TS2307 at `out-w/tests/orders.test.ts(5,25)`.

**1.3 ReDoS.**

```bynk
context web
type Slug = String where Matches("(a|a)+")
service api from http {
  on GET("/s/:slug") (slug: Slug) -> Effect[HttpResult[String]] by v: Visitor { Ok("ok") }
}
```

```sh
bynkc check . && bynkc compile . --target workers -o ow
grep -n 'Slug.of' ow/workers/web/index.ts            # :28, on the raw path segment
for n in 20 24 26 28; do node -e "const s='a'.repeat($n)+'!';const t=Date.now();new RegExp('^(?:(a|a)+)\$').test(s);console.log($n,Date.now()-t,'ms')"; done
sed -n 895,903p bynk-check/src/checker/refinements.rs; gh issue view 724 --json state
```

**Part 2 — #291.** A context with `consumes bynk { Logger }` and
`service ping { on call() -> Effect[Int] given Logger { let _ <- Logger.info("ping") ⏎ Effect.pure(1) } }`;
a suite case `let v <- ping.call()` → `TypeError: Cannot read properties of undefined (reading 'info')`.

**Part 3.** `bynkc check` a project whose `bynk.toml` adds `[dependencies]`, `[deploy]` and
`[workspace]` tables (exit 0), and one with `[paths] incldue = …` (exit 1). For the store rename:
compile `store owner: Cell[String]`, rename it to `ownerName`, recompile (exit 0), then read
`loadState`'s `{ ...__zeroOf…(), ...stored }`. For `Ref`: `fn hold(r: Ref[Counter]) -> Int { 1 }`
→ `unknown type 'Ref'`. For the store-type refinement: `store limit: Cell[Int where Positive] = 1`
→ `expected ']'`.

**Part 4.** Coverage was measured with an instrumented copy of the workspace: `CompileError::new`
logs each code to `$DIAGCOV_LOG`, the copy runs under `cargo test --workspace`, and the log is
diffed against `REGISTRY`. Registry drift: `target/debug/bynk explain bynk.fmt.roundtrip`.
Resolve masking: `commons demo ⏎ fn a() -> Int { "s" } ⏎ fn b() -> Int { nope }` → only
`unknown_name`.

**Part 0.**

```sh
grep -n 'node-version' .github/workflows/ci.yml .github/workflows/release.yml
grep -rn 'typescript@' .github/workflows bynkc/tests/tsc_verify.rs bynk-driver/src/test_runner.rs
grep -n bynkServerVersion vscode-bynk/package.json; gh release view v0.290.1
grep -n NODE_MAJOR_FLOOR bynk-emit/src/lib.rs; grep -n skew bynk/src/test.rs
grep -n 'cargo test --workspace' .github/workflows/release.yml
grep -n 'wasm-bindgen-cli' .github/workflows/*.yml; grep -A1 'name = "wasm-bindgen"$' Cargo.lock
grep -rhn 'design/tracks/' --include='*.rs' . | grep -oE 'design/tracks/[a-z0-9-]+\.md' | sort | uniq -c | sort -rn
```

**Part 6.** Function lengths: brace-matched `fn` extents over comment- and string-masked source,
excluding `#[cfg(test)]` items. Spot-check the top entry with
`awk 'NR>=4255' bynk-emit/src/emitter/emit.rs | …`. Wildcards: `cargo clippy -- -W
clippy::wildcard_enum_match_arm` per crate.
