# The toolchain around the compiler — a review

**Reviewed at:** v0.289.74, 3 September 2026 (`6370cfc`)
**Scope:** the *toolchain* rather than the pipeline — the three shipped binaries (`bynkc`, `bynk`,
`bynkc-lsp`), the external tools they orchestrate (`node`, `tsc`/`tsx`, `wrangler`/`workerd`), the
Rust and Node build pins, the CI gate (`ci.yml`), the release and distribution path (`release.yml`,
GitHub Releases, crates.io, npm, the VS Code extension's server download), and the repo automation
that ties them together (`xtask`, `scripts/bump-version.sh`, `stamp.yml`).
**Reference:** the [30 August 2026 post-restructuring review](2026-08-30-post-restructuring-review.md)
(whose follow-ups this review re-measures in Part 0), `design/bynk-engineering-roadmap.md` Part A,
`design/bynk-release-discipline.md`, and ADR 0083 / ADR 0084 (the driver as thin orchestrator;
`doctor`'s output and exit contract).

Not in scope: the pipeline internals (reviewed four days ago), the language, the formatter's and
the language server's own behaviour beyond how they are built and shipped.

---

## How this review was produced

The tree was built, linted and tested first, on the pinned toolchain (`rustc 1.95.0`), with Node
22.22.2 and `tsc` 6.0.2 on `PATH` and `wrangler@4` provisionable through `npx`:

- `cargo build --workspace --all-targets --locked` — clean (2m21s cold).
- `cargo fmt --all --check` — clean.
- `cargo clippy --workspace --all-targets -- -D warnings` — clean.
- `RUSTDOCFLAGS=-D warnings cargo doc --workspace --no-deps` — clean.
- `BYNK_REQUIRE_TSC=1 cargo test --workspace --locked` — **1,906 passed, 0 failed, 0 ignored**
  across 174 test binaries (5m45s). The three workerd smokes booted and passed; both Node
  strip-types tests ran rather than skipped (one needs Node ≥ 22.6, the other ≥ 22.13 — see §2,
  this matters).
- `cargo xtask greenfield-status` — *table current*, all fifteen gated probes at their recorded
  values.
- The last completed CI run on `main` before this review (`62e988c`, run 4499) is green across all
  27 jobs.

That is the baseline: **no finding below is a broken build, a failing test, or a tripped gate**, and
the tree's own doctor reports every capability but `deploy` as `ok` on this machine. The method was
the one the previous review used — take each claim the toolchain makes about itself and ask the tree
(and, where the claim is about an external tool, the tool) whether it is true. Where a number is
quoted, the command that produces it is in the appendix.

Three experiments were run beyond the suite, because the questions could not be answered by reading:

1. `examples/hello-world` and `examples/orders` were compiled with the tree's `bynkc` and the output
   type-checked under TypeScript **5.9.3**, **6.0.2** and **7.0.2**. All three: zero errors.
2. The whole `tsc_verify` corpus (every project-form positive fixture, plus the embedded runtime
   standalone) was re-run with TypeScript 7.0.2 first on `PATH`: **7 passed, 0 failed** (28s).
3. Node 20.19.5 and 22.22.2 were both asked for `require('node:module').stripTypeScriptTypes`:
   `undefined` on 20, `function` on 22.

---

## Headline

**The Rust half of this toolchain is pinned with unusual care, and the Node half is pinned three
different ways at once.**

Every workflow resolves the same `rustc` by the same action SHA, the `msrv` leg refuses to let the
declared floor drift from the pin, every action is SHA-pinned, the lockfile is `--locked` on every
build that matters, the merge-time stamp mints its push credential only after the last step that
runs third-party code, and a `ci-green` job refuses to be green if any job is missing from its own
`needs:` list. `cargo xtask ci` mirrors the gate locally and the `pre-push` hook runs the cheap half
of it. That is a toolchain that knows what it is standing on.

The tools the *emitted code* stands on are a different story, and the three findings that matter are
all the same finding: a pin chosen for CI's convenience, stated as if it were a contract with users,
and never checked against what users actually get.

**Finding 1 — three TypeScripts.** CI verifies every fixture's emitted output with `typescript@5`
(`ci.yml:288`, 5.9.3 today); the `bynkc test` runner's fallback and `tsc_verify`'s fallback pin the
same major (`test_runner.rs:372`, `tsc_verify.rs:58`). The runtime package, the VS Code extension
and the playground pin **7.0.2** (`bynk-emit/runtime/package.json:15`, `vscode-bynk/package.json:536`,
`playground/package.json:16`). And the remedy `bynk doctor` prints when neither `tsc` nor `tsx` is
found — `npm install -g tsx (or: npm install -g typescript)` (`doctor.rs:317`) — offers two paths,
neither gated: `typescript` resolves to whatever npm's `latest` tag points at, which is **7.0.2**;
`tsx`, the primary suggestion, is an esbuild-based stripper that does no type-checking at all and
is pinned nowhere in the tree. So a user who follows the toolchain's own instructions runs the
emitted TypeScript through a compiler major that no gate has ever run it through, or through no
type-checker at all. `doctor` cannot notice: its `tsc | tsx` row checks presence and provenance
and never reads a version (`doctor.rs:298-333`), though the same file gives `node` a floor. Today
this is a coverage gap, not a defect — experiments 1 and 2 above say the output is clean under 7 —
but it is a gap in exactly the place the `ci.yml:285` comment says the pin exists to protect: *"an
unpinned `typescript` could turn CI red on a new TS major with no repo change"*. Two TS majors have
shipped since that comment was written. The pin has protected CI from noticing.

**Finding 2 — the Node floor is 18, CI runs 20, and the gate that needs 22 has been skipping in CI
for as long as it has existed.** `NODE_MAJOR_FLOOR` is 18 (`bynk-emit/src/lib.rs:94`), a major that
reached end-of-life in April 2025. Nine of the sixteen `setup-node` sites across the workflows —
including both `test` legs, in `ci.yml:283` and `release.yml:150` — run Node **20**, end-of-life
since April 2026. The two strip-types tests in `bynkc/tests/tsc_verify.rs` both skip on Node 20,
for two different reasons. `embedded_runtime_strips_types_under_node` (`:508`) parses `node
--version` against a **≥ 22.6** floor and, if it passes, runs `node --experimental-strip-types
--check` over the runtime (`:511-546`); on 20 the floor fails and the test returns after a banner
(`:531`). `all_emitted_typescript_strips_under_node` (`:578`) stages every fixture's output and hands
the tree to `strip_check.mjs`, which calls `node:module`'s `stripTypeScriptTypes` — an API that
exists from Node **22.13** — and exits 2 when it is missing, which the test treats as another banner
and a pass (`:662`; the earlier banner at `:581` fires only when `node` is absent altogether). Either
way the banner lands in the captured stderr of a passing test, which neither harness prints: nextest's
`success-output` defaults to `never` and `.config/nextest.toml` does not override it, and plain
`cargo test` captures passing output too. Experiment 3 confirms the API is absent on 20. So the
invariant those tests protect — ADR 0136's *"every emitted `.ts` is erasable by pure
type-stripping"*, the property `bynkc test --inspect` and the in-browser eval path both depend on —
has never been checked over the fixture corpus by a CI run. The tree knows: the comment at
`tsc_verify.rs:526-529` says so in as many words and names a compensating control, the Node-22 VS
Code integration job, which runs `bynk test --inspect` over its own handful of debug fixtures. That
is real coverage of a few projects and none of the corpus. Locally, on Node 22, everything passes;
the tree is not broken. But `BYNK_REQUIRE_TSC` and `BYNK_REQUIRE_WORKERD` exist precisely so that a
skip cannot be mistaken for green (`bynkc/tests/require/mod.rs`), and this third external-tool gate
was consciously left without one. Note too that `--inspect` needs Node ≥ 22.6, and source-map-aware
breakpoints ≥ 22.18 (`test_runner.rs:82`, `:701`), so `bynk doctor` says `ok` for a Node on which
`bynkc test --inspect` prints a requirement and exits.

**Finding 3 — nothing has shipped in 35 days, and every VSIX CI builds points at a release that does
not exist.** The most recent GitHub Release, the crates.io `bynkc`, and the npm `tree-sitter-bynk`
are all **v0.245.0** (30 July 2026). The tree is at 0.289.74: 50 commits and 31 increment merges
later. `scripts/bump-version.sh:44` rewrites `bynkServerVersion` on every increment, so
`vscode-bynk/package.json:13` pins `v0.289.74` and `server.ts:179` builds a download URL under
`releases/download/v0.289.74/` — a 404. The `extension` job packages a VSIX from that manifest on
every PR (`ci.yml:577`); the `extension-tests` job passes because it builds the server from source
and points the extension at it. `design/README.md:120` states the invariant — *"the GitHub Release
the extension's `bynkServerVersion` pin points at must exist"* — and nothing checks it; it is true
for exactly as long as no increment merges after a tag. The release-discipline document commits to
*named monthly milestones*; August produced fifty increments and no milestone. This is not a
violation of any written rule (the rule says a tag is cut "when a version is to be shipped"), which
is the problem: the extension's correctness depends on a cadence that no rule or gate enforces.

Two smaller findings follow the same pattern (Part 3), and one design decision deserves revisiting
in the light of Finding 3 (Part 2): `bynk test` is the one command that runs a second, skewable
compiler, and it is the one command that never looks at the skew `doctor` computes.

None of this is an argument that the Node-side pins are wrong. It is an argument that they were
chosen once, for CI, and that the toolchain now states them to users as if they were tested
contracts. The fix in every case is a gate, not a bump.

---

## Part 0 — What the 30 August review asked for, four days later

The previous review closed with eight ordered items. Six landed within four days; the trajectory's
own "delete the wrong shape in the same change" principle (P5) was applied to the two that were
about unadopted code. Re-measured against `6370cfc`:

| # | Ask | Status at `6370cfc` |
|---|---|---|
| 1 | Tag or close `TsExpr::Ident` | **Done** — `TsExpr::VerbatimExpr(String, VerbatimOrigin)` (#1594). 4 `format!`-built `Ident`s remain, all genuine identifiers (`__Commons{type_name}`). `verbatim_origins` 1→2, `verbatim_sites` 2→11, both gated. |
| 2 | Run `verbatim_violations` over compiled output | **Done** — `emitted_typescript_has_no_verbatim_violations` (`bynkc/tests/tsc_verify.rs:401`, #1595), via a new `TsProgram::verbatim_content` walker. Wiring it found two real `as any` casts in the runtime and bindings, fixed at source. |
| 3 | `impl Default for FileId` returning `UNKNOWN` | **Done** (#1584). 95 `Span::default()` sites remain and are now honest. |
| 4 | Decide phase 6's IR: adopt or delete | **Deleted.** `bynk-lower/src/lib.rs` is 2,506 lines (was 10,106); every remaining `pub fn` has a production caller (was 15 without); one `todo!()` (was nine). The IR cutover track recorded the refusal and retired (#1581). |
| 5 | Same decision for phase 8's artefacts | **Deleted** — the definition/project query levels went with #1537; `UnitSignature` stays with three production consumers. The `incremental_query_types` probe now records the deletion in its own reading. |
| 6 | Delete `analyse_project{,_with}`, drop the dev-dep, keep the fixture as a golden test | **Done** (#1541) — `bynk-check` no longer dev-depends on `bynk-emit`; `differential_analysis.rs` is a golden test of `bynk_check::analysis` and says so. |
| 7 | Repoint the 115 dead track citations | **Not done** — **95** remain (`the-ir.md` 42, `semantics-in-the-checker.md` 35, `incrementality.md` 7, `the-typescript-tree.md` 6, `project-model.md` 3, `content-ownership.md` 2). Scheduled as S8 of the crate-hygiene track (#1533), behind an ADR that has not been written. |
| 8 | Add an adoption probe | **Done, for one crate** — `unconsumed_ir_items`, gated at 0 (#1581). It walks `bynk-ir`'s column-zero `pub` items only; `bynk-lower` and `bynk-check/src/queries.rs` are not under it, though with 4 and 5 resolved there is currently nothing there to catch. |

The two ungated trend probes both moved the right way: `keep_in_sync` 240 → **202**, `wildcard_arms`
320 → **310**. The suite shrank from 1,992 to 1,906 tests, which is the deleted IR's own tests
leaving with it — the right direction for a number that was measuring tests of unconsumed code.

The one thing to say about the response is that it was fast and it was P5-shaped, and that the item
left undone (7) is the one that needs an ADR to reverse a standing decision. That is the process
working.

---

## Part 1 — The TypeScript the output is verified with is not the TypeScript users run

### 1.1 The three pins

| Site | Pin | Resolves to today |
|---|---|---|
| `ci.yml:288`, `release.yml:153`, `release-bootstrap.yml:74` | `npm install -g typescript@5` | 5.9.3 |
| `bynkc/tests/tsc_verify.rs:58` (fallback when no `tsc` on `PATH`) | `npx --yes -p typescript@5 tsc` | 5.9.3 |
| `bynk-driver/src/test_runner.rs:372` (`bynkc test`'s own fallback) | `npx --yes -p typescript@5 tsc` | 5.9.3 |
| `bynk-emit/runtime/package.json:15` (the runtime's own typecheck) | `7.0.2` | 7.0.2 |
| `vscode-bynk/package.json:536`, `playground/package.json:16` | `^7.0.2` | 7.0.2 |
| `bynk/src/doctor.rs:317`, `test_runner.rs:506` (the remedies users are told) | `npm install -g tsx (or: npm install -g typescript)` | `tsx` unpinned, no type-check; `typescript` `latest` = **7.0.2** |
| `site/.../contributing/testing.md:62` | documents the `@5` fallback | — |

The runtime package is type-checked by TypeScript 7 in the `runtime` job and then bundled into
`bynk-emit/src/emitter/runtime.ts`, where it is type-checked *with the emitted code* by TypeScript 5
in the `test` job. The same file passes under two majors, which is good — and is also the kind of
fact that is true until it isn't, with no gate positioned to notice which day that is.

### 1.2 What the runner actually does

`bynkc test` tries `tsc` on `PATH` first (`test_runner.rs:371`) and only then the pinned `npx`
fallback. So the pin governs the machine with *no* TypeScript, and the machine with a TypeScript —
the common case, and the case `doctor`'s remedy creates — runs whatever is installed. Two developers
with `bynk doctor` reading `ok` can be running the emitted code through compilers two majors apart.

Experiments 1 and 2 say this is fine today for the whole positive-fixture corpus. That is the
measurement to keep making; this review's argument is only that it is currently made by hand.

### 1.3 What to do

Gate the TypeScript major users get, not the one CI is used to. Concretely: run `tsc_verify` under
`typescript@7` in CI (a second `Install TypeScript` step, or a matrix axis on the Linux leg only —
the corpus takes seconds), keep `@5` if a floor is wanted, and make the three runner/test fallbacks
agree with the remedy. Give `doctor`'s `tsc | tsx` row the same version treatment `node` gets: a
floor, and a *ceiling* until a major has been gated, so `doctor` can say "tsc 8 — untested" the day
it appears rather than `ok`.

---

## Part 2 — Node: a floor of 18, runners on 20, features that need 22

### 2.1 The numbers

| What | Node major | Source |
|---|---|---|
| `NODE_MAJOR_FLOOR` (doctor's floor; "the emitted code targets it") | **18** (EOL April 2025) | `bynk-emit/src/lib.rs:94` |
| CI `test` legs, release `test` legs, `grammar`, `npm-audit`, `playground`, both `release-bootstrap` jobs | **20** (EOL April 2026) | `ci.yml:283,499,567,648,813`; `release.yml:83,150`; `release-bootstrap.yml:68,147` — nine sites |
| `stamp`, `site`, `extension-tests`, `runtime`, npm publish | 22 | the other seven `setup-node` sites |
| `bynkc test --inspect` (`--experimental-strip-types`) | ≥ 22.6 | `test_runner.rs:701` |
| `--inspect` with source-map-resolved breakpoints | ≥ 22.18 | `test_runner.rs:82` |
| `embedded_runtime_strips_types_under_node` (`--experimental-strip-types --check`) | ≥ 22.6 | `tsc_verify.rs:511-523` |
| `all_emitted_typescript_strips_under_node` (`stripTypeScriptTypes`) | ≥ 22.13 | `bynkc/tests/support/strip_check.mjs` |
| `@types/node` the runtime is typed against | 26.2.0 | `bynk-emit/runtime/package.json:16` |

### 2.2 The gate that always skips

`bynkc/tests/tsc_verify.rs` carries two tests that ask Node itself whether emitted TypeScript
strips, and they use two different oracles with two different floors:

- `embedded_runtime_strips_types_under_node` (`:508`) checks the runtime alone with `node
  --experimental-strip-types --check`, behind its own inline `node --version` parse against
  **≥ 22.6** (`:511-523`). Below the floor it prints a banner and returns (`:531`).
- `all_emitted_typescript_strips_under_node` (`:578`) stages every fixture's output and runs
  `strip_check.mjs`, whose oracle is `node:module`'s `stripTypeScriptTypes` (**≥ 22.13**). The
  harness exits 2 when the API is missing and the test prints a banner and passes (`:662`). Its
  other banner (`:581`) is a plain `node`-absent guard and is not taken on the Node 20 legs.

`strip_check.mjs:13-15` explains why the two oracles are not interchangeable: `--check` false-fails
on a leading `type`/`declare` statement, so it is a weaker oracle than the API call — which means
the test that skips for the higher floor is the more trustworthy of the two. On the Node 20 both
`test` legs run, both predicates fail (experiment 3), and both banners land in a passing test's
captured output. Nothing prints that: nextest's `success-output` defaults to `never`, and
`.config/nextest.toml` sets only `failure-output` and `status-level` (which selects the statuses
listed, not what is captured — flipping it to `pass` would list the tests and still hide the
banner); `cargo test` captures passing output too, absent `--nocapture`. So the strip-only
invariant has been certified green on every CI run, under both harnesses, without the corpus ever
being stripped in CI, since the day the tests landed.

The runtime test's own comment (`:526-529`) is candid about this: *"this skips silently on an older
Node regardless of `BYNK_REQUIRE_TSC` (CI's `Test suite` runs Node 20 …). The strip-types coverage
in CI comes from the Node-22 VS Code integration job that runs the emitted `.ts`; this test is the
fast local backstop."* The integration job does run `bynk test --inspect` (`vscode-bynk/src/debug.ts:12`,
`test/suite/debug_*.test.ts`), on its debug fixtures. That covers the runtime and a few projects. The
test that covers *every* fixture is the one that skips. A gate whose documented reason for skipping
in CI is "another job covers it" should name the job in a `BYNK_REQUIRE_*` variable that job sets,
so the claim is checked rather than remembered.

This is the same shape `require/mod.rs` was written to end for `tsc` and `workerd` — *"CI turns that
skip into a hard failure on the OS where the toolchain is expected to work"* — applied to two of the
three external gates and not the third.

### 2.3 What to do

One change closes it: `node-version: "22"` on the `test` legs (and the release's), and a
`BYNK_REQUIRE_STRIP=1` (or folding it under `BYNK_REQUIRE_TSC`, which is what the comment at
`tsc_verify.rs:658` already likens it to) applied at all three skip predicates — the runtime test's
version floor (`:524`), the corpus test's `node`-absent guard (`:579`), and its exit-2 arm
(`:660`) — so the two tests fail rather than skip. Then raise `NODE_MAJOR_FLOOR` to 22: it is what the debug path already requires, it is the
oldest major still in support, and `doctor` stops reporting `ok` for a Node on which
`bynkc test --inspect` cannot run.

---

## Part 3 — Release and distribution

### 3.1 The pin that points at nothing

| Channel | Latest shipped | Tree | Gap |
|---|---|---|---|
| GitHub Releases | `v0.245.0`, 30 July 2026 | 0.289.74 | 50 commits, 31 increments, 35 days |
| crates.io `bynkc` (and the other fifteen) | 0.245.0 | 0.289.74 | same |
| npm `tree-sitter-bynk` | 0.245.0 | 0.289.74 | same |
| `vscode-bynk` `bynkServerVersion` | — | **`v0.289.74`** | points at a release that does not exist |

The extension's resolution order (`server.ts:3-9`) is setting → `PATH` → cache → download, so a
developer with `bynkc-lsp` on `PATH` never notices. Anyone else installing the VSIX that
`ci.yml:577` packages on every PR gets the error at `server.ts:173` — *"Build it (`cargo build
--release -p bynk-lsp`) and set `bynk.executablePath`"* — which is a correct message for a
misconfigured extension and a strange one for a freshly built one.

The root cause is structural, not a missed tag. `bump-version.sh` couples `bynkServerVersion` to the
workspace version on every increment (`:44`), while the design says the pin must name a release that
exists (`design/README.md:120`). Those two statements are consistent only at the instant a tag is
cut. Two ways out, either of which is a gate rather than a habit:

- **Decouple the pin.** Let `bynkServerVersion` name the *last shipped* release, bumped by the
  release workflow (or a release-time step in the same script), so the extension in the tree is
  always installable. The `verify` job's tag/version match (`release.yml:84-92`) would then check
  `pin == tag` rather than `pin == workspace`.
- **Or keep the coupling and check it.** A CI step on the `extension` job that asks GitHub whether
  `releases/tag/$bynkServerVersion` exists, failing when it does not, turns the cadence into a gate.
  The cost is that the first increment after a tag goes red until the next tag — which is exactly
  the information the cadence document says a maintainer wants.

Either way, the milestone cadence in `bynk-release-discipline.md` is the document's promise and
August was its first full month without one. Worth deciding whether that is a slip or a change of
policy; if the latter, the document and the README's `cargo install --path` instructions (which
already quietly route users around crates.io) should say so.

### 3.2 The release runs a different test harness from CI

CI's `test` job runs `cargo nextest run --workspace --locked --profile ci` (`ci.yml:324`): one
process per test, one retry with a fixed 1s backoff (`.config/nextest.toml:27`), flaky-but-passed
tests reported. The release's `test` job runs `cargo test --workspace --locked` (`release.yml:178`):
threads in a shared process, no retry. `bynkc/tests/require/mod.rs:33` notes the difference in
passing — *"`cargo test`, unlike nextest, shares [process state] across concurrently-running
tests"* — and the v0.245.0 release run is the recorded case where it mattered: the wrangler
provisioning race that CI's retry had been absorbing turned the release red (`ci.yml:300`). The
prewarm step fixed that race, in both workflows. The harness difference is still there, and it cuts
both ways: the release is stricter about flakes and weaker about test isolation. A release gate
should run the suite the same way the PR gate does, and the retry policy should be a decision
written in one place rather than a side-effect of which command each workflow happens to call.

### 3.3 Pins whose comment has outlived them

- **`wasm-bindgen-cli@0.2.126`** (`ci.yml:496`, `deploy-playground.yml:193`), with the comment
  *"must match the wasm-bindgen crate (Cargo.lock: 0.2.126)"* (`deploy-playground.yml:189`). The
  lockfile has carried **0.2.127** since #1522 (30 August). The `playground` job passes (39s on run
  4499) because the CLI checks the schema version embedded in the `.wasm`, not the crate version —
  so the comment states a constraint that does not hold, and the day a schema bump makes it hold
  again there is no guard. The wrangler spec has one (`bynkc/tests/wrangler_prewarm.rs` asserts the
  workflows name `wrangler/mod.rs:24`'s `SPEC`); this pin should get the same: a test that reads
  `Cargo.lock`'s `wasm-bindgen` version and the two workflow lines and asserts they agree.
- **`compatibility_date = "2024-11-01"`** (`bynk-emit/src/emitter/wrangler.rs:13`) — every generated
  `wrangler.toml` locks the Workers runtime to a date now twenty-two months old, under a comment that
  says *"bump cautiously"*. Cautious is right; a date with no review trigger is not a policy. Worth a
  line in the release-discipline document: bumped at each milestone, or bumped when the runtime
  package's `@types/node` moves, or never — but stated.
- **`wrangler@4`** floats the minor in the driver (`bynk/src/workers.rs:245`) and the tests
  (`wrangler/mod.rs:24`) while the two deploy workflows pin `4.112.0` exactly (npm's `latest` is
  4.128.0). Deliberate — the `npx` cache hashes the spec — and fine; noted only so the next reader
  does not "fix" the inconsistency in the wrong direction.
- **`fuzz.yml`** resolves `toolchain: nightly` unpinned (`:36`). It is nightly-only, scheduled, and
  off the PR gate, so a nightly regression costs a red scheduled run and nothing else. Acceptable;
  the one workflow that floats should say it floats on purpose.
- **`.config/nextest.toml:25`** parenthesises *"status-level = fail keeps failures loud"* beside
  the claim that retried-then-passed tests are reported as FLAKY. `status-level` chooses which
  statuses are listed; the setting that decides whether a passing test's output is ever seen is
  `success-output`, which the file leaves at its `never` default. The comment is adjacent to the
  truth in the way §2.2 turned out to matter.

---

## Part 4 — The driver's skew check reports, and the one command that could act on it doesn't

`bynk` links the compiler in-process for `check`, `fmt`, `dev` and `deploy` (ADR 0101), so a second
compiler enters on exactly two paths: a `BYNK_BYNKC` override, and `bynk test`, which delegates to
the resolved `bynkc` *always* (`bynk/src/test.rs:1-11`) because it orchestrates `tsc`/`node`
anyway. `compiler::resolve` classifies the resolved binary's skew — patch ignored, minor warns, major
errors (`compiler.rs:48-70`) — and `doctor` renders it. `bynk test` never reads it: `test.rs` passes
`compiler.path` to `shell::delegate` and nothing else, and `workers.rs:18-21` records the decision
for the override path — *"doctor reports its skew only here."*

That decision was taken when patch and minor were both a day apart. Finding 3 changes the odds. A
`cargo install bynkc` from crates.io today is 0.245.0; a `cargo install --path bynk` from the tree is
0.289.74; `PATH` wins over the sibling (`compiler.rs:148-153`), so this pairing runs `bynk check`
through a checker forty-four language increments ahead of the compiler `bynk test` shells. `doctor`
would say `skew: minor — warn`; `bynk test` would say nothing and either reject a program `bynk check`
accepted, or accept one it rejected. `--strict` makes `doctor` fail on it, but `doctor` is a command
a user runs once, and `test` is the one they run all day.

The smallest fix is one `eprintln!` in `test.rs` when `compiler.skew` is `Minor`, and a refusal on
`Major` (the floor `doctor` already calls a "contract mismatch"). A fuller one is ADR 0084's
amendment: `doctor`'s exit contract already distinguishes bare from `--strict`; the same two levels
could apply to the commands that shell out, so the driver's single richest fact about its
environment is used by the command that most needs it.

---

## Part 5 — Things worth recording as correct

Because a review that only lists gaps misdescribes this toolchain:

- **The Rust pin is one fact stated once and checked.** `rust-toolchain.toml` pins `1.95.0`; all six
  workflows resolve it through the same `dtolnay/rust-toolchain` SHA; the `msrv` job's *"declared
  MSRV still equals the pinned dev toolchain"* step makes `rust-version` unable to drift from the
  pin. (It also makes the MSRV claim exactly as strong as the pin and no stronger — "we build on
  1.95" — which the README badge states honestly.)
- **`xtask ci` is the gate, locally, in the gate's own cheapest-first order**, and its `--fast`
  selection is unit-tested so clippy cannot silently fall out of the pre-push hook
  (`xtask/src/main.rs:381`). The `[profile.dev]` debuginfo note in `Cargo.toml` — a measured
  negative result, recorded so it is not re-tried — is the best comment in the repository.
- **The wrangler prewarm is the model for external-tool pins**: one `SPEC` constant, one drift test
  that reads the workflow files, one comment that says which incident it prevents. Findings 1, 2 and
  3.3 are each an argument for applying that model one more time.
- **The `require` contract** (`bynkc/tests/require/mod.rs`) is the right shape — one implementation,
  the empty-string subtlety documented with the two PRs that got it wrong — and Part 2 is only the
  observation that it has two members and needs three.
- **`stamp.yml`'s credential ordering** (mint the bypass token after the last step that runs
  third-party code; stage named paths, never `git add -A`) and `release.yml`'s on-`main` ancestry
  check (with an honest note that it is a partial mitigation) are threat-modelled, not merely
  configured.
- **`ci-green` audits its own `needs:`** against the workflow's job list, so a job cannot be added
  and gate nothing. This is the same idea as an adoption probe, applied to the CI graph.
- **`bynk doctor`** itself: presence, version and *provenance*, with `npx`-provisionable never
  reading as `ok` because it would download on first use. That is a more honest environment check
  than most language toolchains ship. Part 1 asks it to apply its own `node` treatment to `tsc`.

---

## Part 6 — What to do

Ordered by value over cost.

1. **Run the strip-types gate for real.** `node-version: "22"` on the `test` legs in `ci.yml` and
   `release.yml`, plus a `BYNK_REQUIRE_STRIP` (or `BYNK_REQUIRE_TSC` covering it) at each of the
   three skip predicates in `tsc_verify.rs` so the two tests fail instead of skip. Two workflow
   lines and three `require::is_required` calls; it turns an invariant that has never been checked
   over the corpus in CI into one that is.
2. **Verify emitted output on the TypeScript users get.** A second `tsc_verify` pass under
   `typescript@7` on the Linux leg; align the two `@5` fallbacks and the `npm install -g …`
   remedies with whatever the gate says (and decide whether recommending `tsx`, which type-checks
   nothing, is what `doctor` should lead with); give `doctor`'s `tsc | tsx` row a version floor and
   an "untested major" ceiling.
3. **Make the extension's server pin true by construction.** Either decouple `bynkServerVersion`
   from the workspace version (bumped at release time) or add a CI check that the pinned release
   exists. Then decide, in writing, whether the monthly-milestone cadence still stands.
4. **Raise `NODE_MAJOR_FLOOR` to 22** — the oldest supported major and the one `--inspect` needs.
5. **Let `bynk test` read the skew it already has**: warn on minor, refuse on major.
6. **One harness for both test gates.** `nextest` in `release.yml` (it is already installed by the
   same action in CI), and the retry policy stated once.
7. **A drift test for the `wasm-bindgen-cli` pin**, on the `wrangler_prewarm.rs` pattern; fix the
   `deploy-playground.yml:189` comment while there. A written bump policy for `COMPATIBILITY_DATE`.
8. **Repoint the 95 dead track citations** — the one item from 30 August still open, waiting on
   the crate-hygiene track's S0 ADR.

None of this is remediation. The tree is green, every experiment passed, and the response to the
last review was the fastest and most complete this reviewer has seen. What is left is that the
toolchain's contract with the machines that *run* its output is stated in four places, tested in
one, and the one is the oldest.

---

## Appendix — how to reproduce every number

Run from the repository root at `6370cfc`.

**Baseline (all green).**

```sh
cargo build --workspace --all-targets --locked
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps
BYNK_REQUIRE_TSC=1 cargo test --workspace --locked 2>&1 | tee test.log   # 1,906 passed, 174 binaries
cargo run -q -p xtask -- greenfield-status           # 15 gated probes; "table current"
grep -E '^test result' test.log | awk '{p+=$4; f+=$6} END {print NR, p, f}'   # 174 1906 0
```

**The three TypeScripts.**

```sh
grep -rn 'typescript@5' .github/workflows bynkc/tests/tsc_verify.rs bynk-driver/src/test_runner.rs
grep -n '"typescript"' bynk-emit/runtime/package.json vscode-bynk/package.json playground/package.json
grep -n 'npm install -g' bynk/src/doctor.rs bynk-driver/src/test_runner.rs
curl -s https://registry.npmjs.org/typescript | python3 -c 'import json,sys;print(json.load(sys.stdin)["dist-tags"]["latest"])'
```

**Experiment 1 — two examples under three majors.**

```sh
target/debug/bynkc compile examples/orders -o /tmp/orders/out
npx --yes -p typescript@5 tsc -p /tmp/orders/out/tsconfig.json --noEmit   # 5.9.3
tsc -p /tmp/orders/out/tsconfig.json --noEmit                             # 6.0.2 on this host
npx --yes -p typescript@7 tsc -p /tmp/orders/out/tsconfig.json --noEmit   # 7.0.2
```

**Experiment 2 — the whole corpus under 7.0.2.**

```sh
npm install -g --prefix /tmp/ts7 typescript@7.0.2
PATH=/tmp/ts7/bin:$PATH BYNK_REQUIRE_TSC=1 cargo test -p bynkc --test tsc_verify
```

**Experiment 3 — Node 20 has no `stripTypeScriptTypes`.**

```sh
npx -y node@20.19.5 -p "typeof require('node:module').stripTypeScriptTypes"   # undefined
npx -y node@22.22.2 -p "typeof require('node:module').stripTypeScriptTypes"   # function
grep -n 'node-version' .github/workflows/*.yml | sed 's/.*node-version: //' | sort | uniq -c   # 9 × "20", 7 × "22"
grep -n 'SKIPPED' bynkc/tests/tsc_verify.rs                                      # :531 :581 :662
sed -n 511,523p bynkc/tests/tsc_verify.rs                                        # the runtime test's >= 22.6 floor
grep -n 'success-output\|status-level\|failure-output' .config/nextest.toml      # no success-output override
```

**Release drought and the extension pin.**

```sh
git fetch origin tag v0.245.0
git log -1 --format='%ci' v0.245.0                                  # 2026-07-30
git log --oneline v0.245.0..HEAD | wc -l                           # 50
git log --oneline v0.245.0..HEAD | grep -vc 'chore(stamp)'         # 31
curl -s https://crates.io/api/v1/crates/bynkc | python3 -c 'import json,sys;print(json.load(sys.stdin)["crate"]["max_version"])'
curl -s https://registry.npmjs.org/tree-sitter-bynk | python3 -c 'import json,sys;print(json.load(sys.stdin)["dist-tags"])'
grep -n 'bynkServerVersion' vscode-bynk/package.json scripts/bump-version.sh
grep -n 'releases/download' vscode-bynk/src/server.ts
```

**Release harness vs CI harness.**

```sh
grep -n 'cargo test --workspace\|nextest run' .github/workflows/ci.yml .github/workflows/release.yml
sed -n 20,36p .config/nextest.toml
```

**Drifted pins.**

```sh
grep -n 'wasm-bindgen-cli' .github/workflows/ci.yml .github/workflows/deploy-playground.yml
grep -n -A1 'name = "wasm-bindgen"' Cargo.lock                     # 0.2.127
git log --oneline -S'0.2.127' -- Cargo.lock                        # #1522
grep -n 'COMPATIBILITY_DATE' bynk-emit/src/emitter/wrangler.rs
grep -rn 'wrangler@' bynk/src bynkc/tests/wrangler .github/workflows
```

**Skew is computed and not consumed.**

```sh
grep -n 'skew' bynk/src/*.rs | grep -v 'compiler.rs\|doctor.rs\|report.rs\|cli.rs'   # doc comments only
sed -n 14,21p bynk/src/workers.rs
```

**Part 0 re-measurements** (the 30 August appendix's commands, unchanged), plus:

```sh
wc -l bynk-lower/src/lib.rs                                       # 2,506
grep -c 'todo!' bynk-lower/src/lib.rs                             # 1
grep -rhn 'design/tracks/' --include='*.rs' . | grep -oE 'design/tracks/[a-z0-9-]+\.md' | sort | uniq -c | sort -rn
git log --oneline -S'impl Default for FileId' -- bynk-syntax/src/span.rs    # #1584
```

**Last CI run on `main` before this review** — run 4499 (`62e988c`): 27 jobs, all `success` or
`skipped`-by-design; `Test suite (windows-latest)` 419s, `(ubuntu-latest)` 212s, `(macos-latest)`
226s, `Playground (type-check)` 39s.
