# 0271 — `bynkc test --coverage` — V8 line coverage remapped onto `.bynk` source

- **Status:** Accepted (v0.227)

**Context.** Bynk has a first-class test runner — discovery, rich/JSON output,
seeded `property` tests, per-case filtering, an inspector path — but no way to
answer "which lines of my `.bynk` actually ran." A user who wanted coverage had
to hand-wrap the emitted `out-js` and read the numbers against generated
TypeScript (synthetic codec wrappers, capability-injection glue, `Ok`/`Err`
construction) that has no stable relationship to anything they wrote. The runner
already owns the two things a coverage tool needs and a user cannot reconstruct:
it controls the `node` process that executes the tests, and it holds the source
maps from `.bynk` → emitted `.ts` ([[0103]]). Coverage is only useful in `.bynk`
coordinates, so both the collection and the remap belong next to the runner.
This is a tooling-only increment: no grammar / AST / checker / emitter-of-user-
code / runtime change, building on the test-JSON surface ([[0098]]).

**Decision.**

**(A) Collector — V8 built-in coverage, not source instrumentation.** Set
`NODE_V8_COVERAGE=<dir>` on the existing `node` launch; V8 writes per-file
coverage JSON on exit; the runner reads it, filters to the project's emitted
files, and remaps through the source maps. No instrumentation pass, no emitted-
code change, no extra dependency, and it composes with the untouched pass/fail
harness. Istanbul-style instrumentation of the emitted TS would need a transform
pass and a bundled instrumenter and would fight the `tsc`-then-`node` model.

**(B) Attribution unit — line/statement, branch deferred.** A `.bynk` line is
*covered* when a generated line mapping to it executed. Branch coverage in
`.bynk` terms (per `if`/`match` arm) needs a coarser-than-JS notion of a Bynk
branch and is a genuine second design; deferred. A consequence of line-level
attribution: `bynk`'s `if`/`else` lowers to a single-line ternary, so a
never-taken arm on a line that otherwise ran reads as covered — correct for line
coverage, and the motivating case for (B)'s follow-up.

**(C) Runner support — `tsc → node` only.** `--coverage` requires the
`tsc → node` path (the CI-shaped path with real `.js.map`s). `NODE_V8_COVERAGE`
is a Node feature, but the `tsx` fallback's on-the-fly transform muddies which
map applies, and `--inspect` is a debug path with a different purpose. So
`--coverage` errors clearly when combined with `--inspect` or `--no-run`, and
when only `tsx` is available — never producing silently-wrong numbers. The
runner overwrites the emitted `tsconfig.json` with a `sourceMap: true` variant
for the coverage run, kept coverage-only so a normal `bynkc test` or deployment
`tsc` ships no `.js.map`s.

**(D) Measured scope — user `.bynk` source only.** The measured set excludes the
`tests/` tree and the generated workers scaffold (the second, workers-mode
compile a project with integration suites triggers). This is filtered **once**,
authoritatively, on the emitted tree: the executed `.js`'s `out-js`-relative path
whose leading component is `tests/` or `workers/` is dropped before the maps are
consulted. The resolved `.bynk` side is deliberately **not** re-filtered — a user
source that merely lives under a directory named `tests`/`workers` (e.g.
`src/workers/helpers.bynk`) is real code whose `.js` already passed the emitted
filter, so re-filtering by name would silently omit it. So an integration suite
that runs its participants as real Workers attributes nothing to that scaffold; a
unit suite over a commons is what lands.

**The remap** is the one genuinely new piece of logic. `tsc` does *not* chain
input source maps, so attribution is a two-hop, line-level compose: V8 gives byte
ranges into the executed `.js`; `out-js/**/*.js.map` (hop 1) maps a `.js` line to
its emitted `.ts` line; `out/**/*.ts.map` (hop 2, [[0103]]) maps that `.ts` line
to a `.bynk` line. Emitted glue with no `.bynk` origin is *unmapped* in hop 2, so
it contributes nothing — counted as out-of-scope, never as uncovered user code.
A `.bynk` line's verdict is taken from the **tightest** (smallest-span) V8 range
covering any generated position that maps to it: a function body's own range
beats the whole-module range, so a hoisted `exports.f = f;` — which runs at load
and maps back to the declaration line — never masks a never-called function.

**Consequences.** Coverage is a report *about* a run that already happened, so it
never gates: the exit code follows the run's own status, and an unreadable or
partial map degrades the numbers rather than aborting. The `--format json`
document gains an optional, last-position `coverage` block (byte-layout-stable
when absent), extending the [[0098]] family. Node's `NODE_V8_COVERAGE` output
shape is parsed defensively against version drift. Branch coverage (B) and
broader runner support (C) are the recorded follow-ups.
