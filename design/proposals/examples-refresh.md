# Examples refresh — documentation + testable, more representative example code

A docs-and-examples increment (no grammar/compiler/emitter/tooling change → no
version bump, no tag, per [proposals/README](README.md)). It does two things to
the `examples/` gallery:

1. **Correct the documentation** — one of the three "honest limitations" the
   gallery advertises is now stale and actively misleading; fix it and tighten
   the per-example prose.
2. **Refine the example code so more of it is genuinely tested** — without
   inventing structure the language can't yet test. Today only 2 of 7 examples
   ship tests; this brings it to 6 of 7, by separating each example's pure logic
   from its platform-touching shell.

Everything below was verified against `bynkc 0.78.0` (built from this tree):
every refactor compiles, every new test passes, and every refactored example
still compiles to a Worker.

## What's actually true today (the investigation)

The gallery's `README.md` carries three caveats. I checked each against the
compiler:

| Caveat | Verdict | Evidence |
|---|---|---|
| `HttpResult` has no redirect / `429` variant | **Still true — keep** | `bynk-syntax/src/ast.rs` `HTTP_VARIANTS`: 10 variants, none for redirect or 429 |
| `given` can't sit on free functions | **Still true — keep** | `FnDecl` has no `given` field; only `Handler` does |
| capability-consuming code "can't be tested yet" | **Stale & wrong — rewrite** | see below |

The third is the load-bearing finding. The *precise* truth at v0.78:

- A test **can** target a `commons`, a capability-free agent, a user-declared
  `capability` (mockable), or a consumed *context* (mockable via `mocks`).
- A test **cannot** target a context that itself `consumes bynk { … }` /
  `consumes bynk.cloudflare { … }`. Such a context fails to *emit* its test
  surface — `Property 'makeSurface' does not exist` — and this poisons the whole
  context, including any pure helper or capability-free agent inside it. (Proven
  by reduction: lift the capability-free `Limiter` agent out of the platform
  context and it tests cleanly; leave it in and even a pure-function test fails
  to emit.)

So the examples ship without tests not because "handlers with capabilities are
untestable" (the README's framing) but because they consume **platform**
capabilities *in the same context* as their testable logic. The fix is
structural separation, and it needs no new language feature.

This `makeSurface` limitation is real and worth fixing in the compiler, but
that's a separate track. **[DECISION A]** File a GitHub issue documenting it
(minimal repro: a one-context project that `consumes bynk { Logger }` plus any
`test` block → emit failure) and reference it from the gallery README. Do **not**
fix it here.

### Considered and rejected: wrapping platform capabilities

The idiomatic-looking move — wrap `Kv`/`Random` behind a user `capability` +
`provides … given Kv` so the wrapper is mockable — *compiles and the deploy path
works*, but does **not** make the flow testable: the wrapper's provider still
forces `consumes bynk { Kv }` into the context, so the test surface still fails
to emit. It would add a capability/provider layer for representativeness while
delivering zero testability. **[DECISION B]** Out of scope. No example gains a
user-declared capability in this increment. (The `capability`/`provides`/`given`
feature remains undemonstrated by the gallery; noted as a future example idea,
not blocked by anything here.)

## The code delta — separate pure logic from the platform shell

The pattern, four times: move the example's pure, platform-free logic into a
`commons` (or, where the logic is an agent's, behind a pure `commons` helper the
agent calls), `uses` it from the context, and add a `tests/` unit. The HTTP/cron
shell keeps consuming platform capabilities and stays untested (covered by the
issue from DECISION A).

A constraint discovered and designed around: a `commons` sees only base types,
its own types, `Result`/`Option`, and `ValidationError` — **not** platform types
like `Response`/`FetchError`. So extracted helpers take **scalars** at the
commons boundary; the platform-typed `match` stays in the handler. Also: a
`commons` *record* returned from a `fn` carries the commons' nominal brand and
won't unify with a context's mixed-in copy — so extracted helpers return
**primitives**, and the context builds its own record from them.

| Example | Extract into `commons` | Tests added | Worker topology |
|---|---|---|---|
| **rate-limiter** | `commons window` — the pure fixed-window arithmetic as `decide(prevStart, prevCount, now, windowMs, limit) -> Decision` (all-primitive `Decision`); the `Limiter` agent calls it and builds `RateView` itself | window math: first-hit allows+counts, over-limit denies+doesn't-count, lapse opens a fresh window | **unchanged — one worker** |
| **uptime-monitor** | `commons status` — `Status`, `statusKey`, and pure `statusFor(name, at, code: Int) -> Status` (`code == 0` ⇒ unreachable ⇒ unhealthy); cron handler maps `match res { Ok(r) => statusFor(_, _, r.status); Err(_) => statusFor(_, _, 0) }` | healthy 2xx, unhealthy 5xx, unreachable (code 0), key namespacing | unchanged |
| **feature-flags** | `commons keys` — `FlagKey` refined type + `keyOf`/`nameOf` | boundary: empty `FlagKey.of` is `Err`, valid is `Ok`; `keyOf`→`nameOf` round-trips | unchanged |
| **link-shortener** | `commons codes` — `Slug` + `Url` refined types + `keyOf` | boundary: under/over-length `Slug.of`, malformed/oversize `Url.of`; `keyOf` namespacing | unchanged |

Unchanged code:

- **hello-world**, **todo** — already ship tests (a `commons` and a
  capability-free agent respectively). They *are* the model this increment
  applies elsewhere; left as-is.
- **webhook-relay** — every step is effectful at the boundary (HMAC verify →
  `Fetch` → `Secrets`); there is no pure kernel worth extracting. **[DECISION C]**
  Stays test-free; its README says why, pointing at the DECISION-A issue. Adding
  a contrived helper purely to have a test would misrepresent the example.

## The docs delta

- **`examples/README.md`** — rewrite the "Notes on the current language surface"
  third bullet to the precise truth above (commons / capability-free agents /
  user capabilities / consumed contexts are testable; platform-consuming
  contexts can't host tests yet → link the DECISION-A issue). Keep bullets #1/#2
  verbatim (still true). Refresh the gallery table's framing so the testing story
  is honest per row.
- **Per-example READMEs** (the four refactored + webhook-relay) — add/refresh a
  **"Check and test"** section with the real `bynkc test .` output, and a
  one-line "what's tested, and what isn't (yet)" note. Update the layout trees to
  show the new `commons` file and `tests/` unit.
- **`hello-world/bynk.toml`** — add the `[project] name/version` block the other
  six manifests carry (consistency; it's the only one missing it).
- **Book** — the examples are reader-facing surface. Add a `reference/changelog.md`
  entry (examples refresh; no version change). Audit the testing guide
  (`docs/src/guides/.../testing*`) and the tutorials for any claim that the
  example projects are untested or that capability-consuming code is untestable,
  and correct it to match the DECISION-A framing. **[DECISION D]** No new book
  *page*; this is a currency-and-consistency pass on existing pages plus the
  changelog. If the audit turns up a guide section that materially misstates the
  testing model, fix it in place and note it in the PR.

## Risks & mitigations

- **Refactor changes runtime behaviour.** Mitigated: each refactor is a pure
  extraction (same arithmetic / same status mapping / same key strings); all four
  examples were re-compiled to Workers after the change, and the extracted logic
  is now pinned by tests. `rate-limiter` deliberately keeps its single-worker
  topology (the `decide` helper is a `commons`, not a second context).
- **The gallery starts demonstrating `commons`-heavy structure.** Acceptable —
  factoring a pure kernel into `commons` is idiomatic Bynk (hello-world already
  does it), and the READMEs explain the *why* (testability at the boundary).
- **Stale framing creeps back.** The new caveat text names the DECISION-A issue,
  so a future reader (or the issue's closer) has a single anchor to update when
  the compiler limitation is lifted.

## Done when

- All 7 examples `bynkc check` clean; the 6 with tests pass `bynkc test .`; all 7
  `bynkc compile --target workers` clean. `rate-limiter` still emits exactly one
  worker.
- `examples/README.md` testing caveat is precise and links the filed issue;
  bullets #1/#2 unchanged.
- The four refactored examples each have a `commons` kernel + a passing `tests/`
  unit, and a README "Check and test" section with real output.
- `hello-world/bynk.toml` has a `[project]` block.
- The DECISION-A issue exists and is linked from the gallery README.
- Changelog entry added; no version bump, no tag; book testing-claims audited and
  corrected where wrong.
