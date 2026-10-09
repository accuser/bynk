# Unit-test sufficiency per crate — a review

**Reviewed at:** v0.314.0, 9 October 2026 (`cb6514ec`)
**Question:** does each crate have enough unit testing of its own, or is the workspace
over-relying on `bynkc/tests` (integration tests by any reasonable definition)? Prompted by a run
of defects that reached `main` and were found afterwards.

---

## How this review was produced

- **Static counts:** `#[test]` functions per crate and per file, split into `src/` (unit) and
  `tests/` (integration).
- **Coverage, two ways**, with `cargo-llvm-cov 0.9.1` on the pinned toolchain (Node 24.15, `tsc`
  on `PATH`; the instrumented suite ran **1,924 passed, 0 failed**):
  - **own**: `cargo llvm-cov -p <crate> --lib`, counting only that crate's files. This is what
    the crate's own unit tests reach.
  - **full**: `cargo llvm-cov --workspace --exclude xtask`, i.e. everything, including
    `bynkc/tests`.
  - **own unit + own `tests/`**: `cargo llvm-cov -p <crate>`, without `--lib`. Measured for the
    four crates with sizeable `tests/` directories (`bynk-fmt`, `bynk-lsp`, `bynk`,
    `bynk-driver`).
  - The gap between *own* and *full* is code that only some other crate's tests reach, which in
    practice means `bynkc`'s fixture suites.
  - Caveat: line counts include each file's own `#[cfg(test)]` module, which slightly inflates
    *own*. The gaps below are still large enough that this doesn't change any conclusion.
- **Defect history:** every non-chore commit since 25 September, classified by which crates' `src/`
  it changed and where it added tests. Every issue opened since 25 September, grouped by defect
  class.
- Not done: mutation testing. A mutation run over `bynk-check`/`bynk-emit` would give the
  strongest evidence for §4's claim and is the obvious follow-up.

---

## 1. Verdict per crate

| Crate | Lines (instr.) | Tests (`src/` + own `tests/`) | Own unit | Own unit + own `tests/` | Full | Verdict |
|---|---:|---:|---:|---:|---:|---|
| `bynk-ts` | 5,159 | 168 | **95.0%** | = | 98.7% | **Sufficient.** Printer is exhaustively unit-tested. |
| `bynk-ide` | 5,854 | 143 + 3 | **91.6%** | — | 93.0% | **Sufficient.** |
| `bynk-lower` | 1,545 | 34 | **85.8%** | = | 95.5% | **Sufficient.** |
| `bynk-lsp` | 8,589 | 128 + 80 | 74.2% | **83.4%** | 83.4% | **Mostly adequate.** Self-contained: none of its coverage comes from `bynkc`. Gaps: `hover.rs` 0%, `capability_fixes.rs` 0%, `wire_contract_request.rs` 6.5%. Open #1819 (extract-variable scope) is a walk bug in `extract.rs`. |
| `bynk-project` | 1,816 | 74 | 66.5% | = | 97.8% | **Adequate, one hole:** `discovery.rs` 13% own. |
| `bynk-fmt` | 2,975 | 52 + 38 (golden, round-trip, proptest) | 60.3% | **93.3%** | 94.0% | **Coverage self-contained; insufficient for its defect class** (§3.2). Comment-slot testing isn't systematic. |
| `bynk` (CLI) | 5,071 | 124 + 80 | 58.6% | **72.8%** | 72.8% | **Adequate for a CLI.** Self-contained. `doctor.rs` 3.6% own; the low full figure is mostly deploy/dev paths that need live tools. |
| `bynk-syntax` | 8,139 | 118 | 46.0% | = | 92.6% | **Thin.** `parser/declarations.rs` 24%, `parser/types.rs` 34%, `ast.rs` 15% own. The parser is mostly tested via `bynkc` negative fixtures. |
| `bynk-driver` | 2,243 | 49 + 5 | 36.3% | 37.6% | 84.6% | **Thin.** Genuinely leans on `bynkc`. `test_runner.rs` 0% own, 64% full. |
| `bynk-emit` | 25,608 | 189 + 12 | 38.6% | —² | 95.6% | **Insufficient.** See §2. |
| `bynk-check` | 28,924 | 183 + 8 | **24.9%** | —² | 88.5% | **Insufficient.** See §2. |
| `bynk-ir` | 70 | 0 | — | — | 100% | Fine: data types only. |
| `bynk-render`, `bynk-strip`, `bynk-wasm`, `bynk-grammar` | small | 7–18 each | n/a¹ | n/a¹ | 86–95% | Fine. |
| `bynkc` | 249 | 0 | — | — | 60% | Fine: a thin CLI. Its `tests/` *is* the integration suite. |

¹ Not measured separately. They're small and their tests sit next to the code they cover.

² Not measured. Their own `tests/` hold only 8 and 12 tests respectively, so they can't account for
much of the gap; `bynkc` and the other crates' tests are what reach this code.

`=` means the crate has no `tests/` directory. `—` means not measured.

**Your instinct is right, but the problem is concentrated.** The two largest crates, `bynk-check`
and `bynk-emit`, are ~55k of the ~100k instrumented lines. That's where nearly all recent defects
live, and their own tests reach only a quarter to two-fifths of their code. Everywhere else is
broadly fine.

---

## 2. Where the gap is: `bynk-check` and `bynk-emit`

These files are reached **only** through other crates' tests. Each is ≥150 lines; sorted by
uncovered-by-own lines:

| File | Lines | Own | Full |
|---|---:|---:|---:|
| `bynk-emit/src/emitter/emit.rs` | 4,913 | 28.5% | 98.0% |
| `bynk-emit/src/project/tests_emit.rs`³ | 4,708 | 38.2% | 95.7% |
| `bynk-check/src/checker/calls.rs` | 2,840 | **0.0%** | 84.6% |
| `bynk-check/src/context_checks.rs` | 3,286 | 22.8% | 91.3% |
| `bynk-check/src/project_model.rs` | 2,526 | 9.4% | 94.3% |
| `bynk-emit/src/emitter/lower.rs` | 3,968 | 37.7% | 90.4% |
| `bynk-check/src/checker/expressions.rs` | 2,857 | 11.0% | 84.3% |
| `bynk-check/src/test_suites.rs` | 2,012 | 3.2% | 89.6% |
| `bynk-emit/src/emitter/workers_entry.rs` | 1,592 | **0.0%** | 98.6% |
| `bynk-emit/src/emitter/workers.rs` | 1,607 | **0.0%** | 93.5% |
| `bynk-check/src/checker/kernels.rs` | 1,843 | **0.0%** | 76.2% |
| `bynk-check/src/resolver.rs` | 1,862 | 10.6% | 84.6% |
| `bynk-check/src/check_pipeline.rs` | 330 | 0.0% | 99.7% |
| `bynk-check/src/analysis.rs` | 327 | 0.0% | 90.5% |
| `bynk-check/src/checker/equality.rs` | 291 | 3.1% | 77.3% |

³ Production code (it emits test suites), despite the name.

**Root cause in tooling.** `bynk-check`'s only source-string harness is the
`checked_context_program` helper, private to `context_checks.rs`. Per an earlier finding, it only
supports what someone has wired into its `UnitTable`. `bynk-emit` has `testkit::emit_source` /
`emit_bundle`, but nothing for the Workers target or for test-suite emission. When the cheapest
way to test a checker or Workers change is a `bynkc` fixture, fixtures are what gets written.

---

## 3. What the defects say

### 3.1 How fixes are tested

47 non-chore PRs since 25 September changed `bynk-check` or `bynk-emit` source. **33 of them
added no `#[test]` anywhere under `src/`.** They were tested only through `bynkc` fixtures, or
(for the fmt fixes that touched check/emit incidentally) through `bynk-fmt/tests`. Examples:
#1813 (63 fixture files changed), #1743 (358), #1698 (957).

The 14 that did add unit tests mostly added one to three, e.g. #1816 and #1766 (3 each). The
outliers are #1689 (16) and #1770 (11).

A golden fixture pins the corrected output for one program. It doesn't generalise to the sibling
case. That's the mechanism behind the next section.

### 3.2 Defect classes since 25 September

| Class | Issues | Where | What would have caught it |
|---|---|---|---|
| **Emitted TS names a type it never imports** (TS2304/TS2552) | #1736, #1778, #1815, #1818, #1823, #1829 (4 still open) | `bynk-emit`: about 21 sites across 6 files write `import` statements, each working out its own needs | One invariant (R1) |
| **Workers entry / composition wrong** | #1817, #1822, #1825, plus the codec/import items above | `workers.rs`, `workers_entry.rs` (0% own) | Unit tests on those files (R3) |
| **A walk skips a child** | #1700, #1760, #1769, #1800, #1819 (open) | ~13 hand-rolled `ExprKind::Match` walks in check/emit/lower/lsp/ide besides `ast::expr_children` | One exhaustiveness test (R2) |
| **Name/type resolves in the wrong scope** | #1807, #1814, #1824 (2 open) | `resolver.rs`, `project_model.rs`, `test_suites.rs` (≤10% own) | Resolver unit tests over a multi-unit table (R4) |
| **fmt refuses or mangles a comment** | #1786, #1788, #1794, #1797, #1808 (open), #1810 (open) | `bynk-fmt` | Comment injected at every slot (R5) |
| **Checker accepts what the spec rejects** | #1781, #1784, #1820 (open), #1821 (open) | `bynk-check` | Negative fixtures *are* the right tool here; spec-to-rule traceability is the gap, not unit tests |

Five of the six classes are **families**. The same bug recurs in a sibling construct, because each
fix pinned one instance.

### 3.3 Why `tsc_verify` didn't catch the TS2304 class

Two oracles type-check emitted TypeScript:

- **`tsc_verify`** type-checks every positive fixture under `--strict`, including the 107 Workers
  fixtures. Since #1767 it covers single-file fixtures too. It compiles each fixture **once**,
  through `compile_project`, for that fixture's own target.
- **`bynkc test`** emits a different layout (`bynk-driver/src/test_runner.rs`). It compiles for
  the bundle target, and when a project has integration suites it also compiles for Workers and
  overlays that output. `tsc_verify` never sees that composed layout. It is type-checked only by
  `behaviour_fixtures.rs`, which drives `bynkc test` over the 102 fixtures that opt in with
  `expected_run.txt`. A `tsc` failure there surfaces as a run error, not as a type-check result.

So oracles exist for both paths. But each can only check combinations someone wrote a fixture
for, and the `bynkc test` path is narrower. The open import bugs are all combinations nobody had
written yet: an agent state codec × a used-commons refined type that the context never reads,
under `bynkc test` (#1829); a cross-context argument whose type the caller's own signatures don't
mention (#1823). **Line coverage of these files is 93–99%.** The lines run. What's missing is an
assertion that generalises across inputs.

### 3.4 This is not mainly a line-coverage problem

Full-suite coverage is high: 88.5% for `bynk-check`, 95.6% for `bynk-emit`. Adding unit tests
that re-walk the same lines with ad-hoc examples will raise the *own* column and catch little.
What has been missing are **property/invariant tests that quantify over a class**: every slot,
every walk, every emitted name. Those are cheap to write at unit level and impractical as `bynkc`
fixtures.

---

## 4. Recommendations (ranked by defects-per-effort)

**R1. Import-closure invariant for emitted TypeScript (`bynk-emit`). (#1831)** Collect the identifiers an
emitted module references in type and value position. Assert each one is declared locally,
imported, or a known global. Run it over every module the positive corpus emits, for both targets
*and* for `bynkc test`'s composed layout.

The identifiers are mostly available from `bynk-ts`'s tree. `Verbatim`/`VerbatimExpr` leaves
carry raw text, so they need a text scan; `bynk-ts/src/lint.rs` already scans those leaves for
another purpose and is the model.

A cheap first step: add `bynkc test`'s composed layout to `tsc_verify` for every suite-bearing
fixture. A stronger form: *derive* the import set from the reference set, instead of predicting
it at the ~21 separate import-writing sites, so this class can't happen.

Would have caught or prevented #1736, #1778, #1815, #1818, #1823, #1829.

**R2. Walk-exhaustiveness test (`bynk-syntax` plus each walk's crate). (#1832)** Build a synthetic AST in
which every `Expr`-holding slot of every `ExprKind`/`Statement` variant holds a distinct marker.
Assert each named walk reaches every marker: `expr_children`, linearity, `body_writes_state`,
`block_uses_send`, visibility, extract-variable, and the rest. Better still, route the remaining
hand-rolled walks through `expr_children` so there's only one thing to test. A single test would
have caught #1700, #1760, #1769, #1800 and #1819.

**R3. A real `bynk-check` / `bynk-emit` unit harness. (#1830)** Promote `checked_context_program` into a
shared `#[cfg(test)]` testkit that takes multi-unit sources (context + commons + uses) and
returns the checked program and diagnostics. Add Workers and test-suite variants of
`emit_source`. Then write a first batch of unit tests for the 0%-own files: `checker/calls.rs`,
`checker/kernels.rs`, `workers.rs`, `workers_entry.rs`, `check_pipeline.rs`, `analysis.rs`. Do
this before mandating unit tests, or the mandate will produce fixtures anyway.

**R4. Resolver scope tests (`bynk-check`). (#1833)** Use R3's harness for a table of shadowing and
transitive-reach scenarios: local vs used-commons type, a type reached two `uses` deep, a test
suite's type table. Assert what each name resolves to, not what gets emitted. Targets #1807,
#1814, #1824.

**R5. fmt comment-at-every-slot test (`bynk-fmt`). (#1834)** For every positive fixture, insert `-- c` at
each line end and each token boundary in turn. Assert that the formatter either keeps the comment
and is idempotent, or refuses with the named diagnostic, and that it never silently drops one.
`round_trip_preserves_injected_comments` does this today for five hand-picked positions. Would
have caught #1786, #1788, #1794, #1797 and #1808 in one go.

**R6. Process: a fix adds a test of the class, not just the instance.** In the pending-file /
PR template, ask: *"What sibling constructs share this bug's shape, and which test covers them?"*
Prefer a unit or invariant test over a fixture when the answer is "more than one". Fixtures stay
the right tool for end-to-end behaviour and for spec-acceptance (negative) cases.

**R7. Smaller holes, low priority:** `bynk-driver/src/test_runner.rs` (0% own, 64% full),
`bynk-syntax` declaration and type parsers, `bynk-lsp` `hover.rs` / `capability_fixes.rs`,
`bynk/src/doctor.rs`, `bynk-project/src/discovery.rs`.

**Follow-up measurement:** run `cargo-mutants` over `bynk-check/src/resolver.rs` and
`bynk-emit/src/emitter/workers.rs`. A high surviving-mutant rate at 90%+ line coverage would
directly confirm §3.4.
