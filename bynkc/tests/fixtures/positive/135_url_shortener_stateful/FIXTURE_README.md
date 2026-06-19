# Fixture: `135_url_shortener_stateful`

A stateful URL-shortener — the richer counterpart to `134_url_shortener`. Where
134 has no agents, this one carries two stateful agents (`Hits` keyed by code
with `count: Int`; `Link` keyed by code with `target: Option[Url]`), a
cross-context `consumes` edge, a capability with provider and mocks, and an HTTP
surface. Its purpose as a fixture is twofold:

1. A second, richer integration regression for v0.9.1's three items (split-paths
   rooting, `assert` as an expression, tsc verification) on a project that
   actually exercises agent state, cross-context calls, and the ACL pattern.
2. **The agent-state-initialisation experiment.** This is the runtime question
   that has outlasted ten findings and a hardening increment, because nothing in
   the pipeline before *executing* a test could reach it.

## Placement

Drop the directory in at `tests/fixtures/positive/135_url_shortener_stateful/`,
alongside `134_url_shortener`. Layout (split-mode, plain `.bynk` under both
roots per the v0.9.1 convention):

```
135_url_shortener_stateful/
├── bynk.toml                       [paths] src = "src", tests = "tests"
├── src/shortener/{core,analytics,links}.bynk
└── tests/shortener/{analytics,links}.bynk
```

## What it must satisfy

- `bynkc compile --target bundle --output out src` — clean.
- `bynkc compile --target workers --output out src` — clean.
- `tsc --strict --noEmit` over the emitted output (both targets) — clean. (This
  fixture exercises cross-context projection and module re-exports, the two
  areas where the v0.9.1 tsc stage already found bugs — so it's a strong guard.)
- `bynkc test` — runs all tests via Node.

## The agent-state-initialisation experiment

Every test starts with a fresh agent key, so the whole suite implicitly depends
on how a never-seen agent initialises its state. Two tests isolate it cleanly,
one per state-field kind:

| Probe test | Agent | Field | Passes iff |
|---|---|---|---|
| `a fresh Hits key reads count as 0` | `Hits` | `count: Int` | fresh Int initialises to `0` |
| `a fresh Link key resolves to NotFound` | `Link` | `target: Option[Url]` | fresh Option initialises to `None` |

**Reading the outcome:**

- **All green** → fresh agent state zero-initialises (Int → 0, Option → None).
  The hypothesis the example was written against holds; document it as the
  defined behaviour and add it to the agent/state spec, which currently does not
  state it.
- **A probe's assertion fails** (test runs, returns the wrong answer) → fresh
  state is *not* zero-initialised. Most likely the field reads back `undefined`
  (so `undefined == 0` is false; `undefined` doesn't match `None`). Agents then
  need an explicit initialiser, or handlers must treat unset state as a distinct
  case.
- **A probe errors/throws** (test doesn't complete) → `loadState` rejects a
  missing key outright, rather than returning a default. Agents would need
  explicit construction before first use.

Whichever of the three occurs is **finding #10**, established by demonstration
rather than by reading code. It directly informs the agent/state portion of the
spec, which has never pinned this down — and it gates any realistic stateful
program, since almost every agent is read at a key before it's first written.

## Notes for whoever runs this

- Refined values are still constructed with `T.of(...)` + match-unwrap; finding
  #7 (refined-construction ergonomics) is not yet addressed, so the tests are
  more verbose than they will eventually be. Don't "fix" that here — it's the
  next increment.
- Asserts use the v0.9.1 expression form (`Ok(x) => assert x == 1`), so this
  fixture also demonstrates that change in anger.
- If the probes reveal non-zero-init behaviour, **do not** silently adjust the
  example to paper over it — record finding #10 first, then decide whether the
  fix is a language/runtime change (define zero-init) or an example change
  (explicit initialisers). The point of the fixture is to surface the truth, not
  to be made green at any cost.
