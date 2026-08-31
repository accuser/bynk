# Post-trajectory crate hygiene — READMEs, tests, comments, module splits

- **Status:** Draft (settling). Direction not yet merged; no slice authorised.
- **Realises:** `design/bynk-greenfield-compiler.md` R10.1 ("a crate is named for
  what it produces … a contributor reading the crate docs will be misled about
  where to make a change" — extended here to READMEs and file structure, not
  just names), R11.1/P1 ("every phase is a total function from a value to a
  value … testability is determined by API shape, not by discipline"), and
  R11.5 (Rust-side coverage instrumented and reported — already true via
  `cargo llvm-cov`, but report-only and never aimed at a target).
- **Posture:** Feature track per [ADR 0076](../decisions/0076-feature-track-posture.md).
  Qualifies on **multi-increment** (four independent workstreams across twenty
  crates, sequenced against a shared CI probe constraint) and **surface not yet
  settled** (the comment-provenance policy reverses a standing decision; the
  `ast_importers` probe amendment needs the same open-review treatment phases 6
  and 7 gave their own probe floors). Not a security/safety boundary — no
  language surface, runtime behaviour, or capability boundary changes.
- **What's already true:** the trajectory closed 30 August 2026 (spine
  [#1507](https://github.com/accuser/bynk/issues/1507)); this is not "phase 9" —
  its theme is the spec-in-place (`bynk-greenfield-compiler.md`), not the closed
  trajectory document. The gated-probe harness (`xtask/src/greenfield_status.rs`,
  `design/greenfield-status.md`) already exists and is the mechanism every
  slice below is checked against; this track adds no new CI infrastructure of
  its own except where named. Spine [#1533](https://github.com/accuser/bynk/issues/1533).

## 1. The theme

Eight phases of structural migration got the compiler onto the architecture
`bynk-greenfield-compiler.md` specifies, and each phase's own probe measured
exactly what that phase moved — never the surrounding hygiene. Four kinds of
residue accumulated that no phase probe owned:

1. **Eight of twenty crates have no README** (`bynk-ts`, `bynk-ir`, `bynk-lower`,
   `bynk-project`, `bynk-driver`, `bynk-testkit`, `tree-sitter-bynk`, `xtask`) —
   five of them crates.io-publishable, rendering a bare docs.rs/crates.io page
   today. Seven of the twelve that do have one carry a hand-drawn dependency
   diagram the migration made false: it still shows `bynk-emit ◀── bynk-ide`, an
   edge phase 4 removed (R10.2; `ide_emit_edge` reads *absent*), and omits five
   crates created since — `bynk-project`, `bynk-ir`, `bynk-lower`, `bynk-ts`,
   `bynk-driver`. One fact, seven hand-synced copies, drifted — exactly the
   failure mode P2 names.
2. **Crate-local unit testing is thinnest exactly where the code is densest.**
   `bynk-ir` 0%, `bynk-syntax` 10.7%, `bynk-check` 10.4%, `bynk-emit` 11.8% with
   no `tests/` directory at all (per `design/greenfield-status.md`'s live
   `test_density` trend row). ~23,900 lines sit in files carrying no in-file
   test at all.
3. **Comments narrate the migration rather than the code.** A census of Rust
   source under every crate's `src/` found ~5,513 comment lines (12% of all
   comment lines in the workspace, present in 88% of files) carrying a slice ID,
   a phase number, an issue number, or a citation to a since-deleted track doc.
4. **Large modules were never re-cut.** `bynk-lower/src/lib.rs` is 10,106 lines
   in the crate's only file; twenty files across the workspace exceed 2,000
   lines, and only four of the thirty largest have been decomposed into
   submodule directories at all.

**End state when this track retires:** every crate has a README following the
one template the workspace already uses; the dependency diagram is derived from
the manifests rather than copied by hand; the crates whose API shape supports it
(per R11.1) have proportionate in-file test coverage, and the two crates whose
low density traced to a real *signature* defect (`bynk-ir`'s dependency on
`bynk-check`) have that defect fixed rather than papered over with tests; the
comment corpus states invariants and constraints rather than edit history; and
every module on the safe-and-carved lists below is cut along cohesion seams, not
line-count thresholds. A contributor arriving cold can find the crate they need,
read why the code is the way it is, change it in a file small enough to hold in
their head, and prove the change at crate granularity.

**What this track deliberately does not do.** It does not touch the five
emitter files (`bynk-emit/src/emitter.rs`, `emitter/emit.rs`, `emitter/lower.rs`,
`emitter/workers.rs`, `emitter/workers_entry.rs`) or the two
`AST_IMPORTER_EXCEPTIONS` files (`project/tests_emit.rs`,
`emitter/serialisation.rs`) — 34,244 lines that are simultaneously the biggest
item-4 targets in the workspace and exactly the `ast_importers` gated-probe
floor argued file-by-file at phase 6's own retirement. §5 names the amendment
that subtree needs; landing it is deliberately a separate, later slice with its
own settling review, per the same discipline phases 6 and 7 both used for their
own probe-floor arguments — not folded into this settling pass.

## 2. Why a track (the ADR 0076 trigger)

**Multi-increment.** Four workstreams (READMEs, tests, comments, splits) across
twenty crates, sequenced against a shared, hard constraint (§3 below) that a
single delete-on-merge increment proposal cannot carry: the CI probe suite gates
14 numbers as an all-or-nothing equality diff, several of which more than one
workstream can move, and getting the sequencing wrong fails CI in a way that
does not localise to the PR that broke it.

**Surface not yet settled.** Two genuinely open questions, both load-bearing:

- The comment-provenance policy this track adopts reverses ADR 0384 and ADR
  0411's explicit, twice-repeated posture — "citations … left as historical
  references, not swept." Reversing a standing decision needs its own argued
  ADR, not a quiet PR description.
- The `ast_importers` probe currently counts *files*. The subtree it gates is
  also the workspace's biggest module-split target, and a naive split can pass
  the gate via a loophole (§5) without fixing anything. The amendment needs
  open review before any code moves, the same way phase 6 and phase 7 each
  argued their own probe floors on the record before relying on them.

**Not a security/safety boundary.** No language surface, runtime behaviour, or
capability boundary changes. This is internal documentation, tests, comments,
and file structure — the same posture `compiler-architecture.md`,
`crate-decomposition.md`, and `content-ownership.md` each took as internal-
architecture tracks rather than language-surface ones.

## 3. The constraint every slice is checked against: the gated probes

`xtask/src/greenfield_status.rs` computes 19 probes; `design/greenfield-status.md`
is the committed table. **Fourteen are gated** — `gated_disagreements`
(`xtask/src/greenfield_status.rs:2455`) does exact string equality per row
against that table, and `xtask/tests/greenfield_status.rs` fails CI (both the
`test` and `drift` jobs) on any disagreement. Three of this track's four
workstreams can move one of them:

| Probe | Reads today | Counting unit | Moved by |
|---|---|---|---|
| `ast_importers` | 5 | **files** in `bynk-emit/src` importing `bynk_syntax::ast` | splits — **deferred to §10/S10, not this track** |
| `emit_diagnostics` | `bynk-emit=4/6`, `bynk-check=389/397` | **distinct** `"bynk.*"` literals per crate `src/`; **no test-range exclusion** | tests (S6) |
| `fs_below_driver` | 0 files | files touching `std::fs` below the driver | splits, via malformed test modules (S4, S9) |
| `options_sources` | present | `struct CompileOptions` with `sources`, `bynk-emit/src/project.rs` | splits (S9) |
| `ts_writes` / `ts_any` | 809 / 26 | `write!`/`writeln!`/`format!` (`ts_writes`) or `: any`-shaped (`ts_any`) occurrences in `bynk-emit/src`, test ranges excluded — **call sites, not the `out: &mut String`/sink-signature count** | splits, via malformed test modules (S9); **also S6** — any test helper added to `bynk-emit` that calls `format!` outside a canonical `#[cfg(test)] mod` block adds to it, Trap A applied to this probe rather than `fs_below_driver` |
| `span_keyed_maps` | 4 | `HashMap<Span` across all `src/`, **comments included** | comments (S8), splits (S4) |
| `workspace_lints` | present | **first line** of root `Cargo.toml` naming the lint | comments (S8) |
| `test_density` | trend only | non-comment lines in trailing `#[cfg(test)] mod`, `src/` only | tests (S5–S7), splits (S4, S9) |
| `keep_in_sync` | trend, 239 | comment lines matching "in sync"/"mirrors"/"parity"/"must match" | READMEs (S2), comments (S8) |

`hoist_sinks` (gated, reads 0 — the `stmts: &mut Vec<String>` sink Tier B/T2.1
already deleted) is omitted from this table deliberately, checked rather than
assumed: its needle names no file this track plans to touch, its count
excludes comments, and nothing here reintroduces that specific signature —
safe to leave gated and unmentioned.

Three traps, all verified in the probe source, that every slice touching
`bynk-emit`, `bynk-ide`, `bynk-fmt`, `bynk-check`, or comments must check
against before committing:

**A. `test_mod_ranges` recognises exactly one form** (`:1012`): a line trimming
to exactly `#[cfg(test)]`, then a next non-empty line that both
`starts_with("mod ")` and `ends_with('{')`. A module written as
`#[cfg(test)] mod tests {` on one line is **invisible** to the probe — every
`write!`/`: any` inside it counts as production, inflating `ts_writes`/`ts_any`.
`bynk-emit`'s only `std::fs` uses sit inside `project.rs`'s trailing test block,
so a malformed module there **flips `fs_below_driver` off its gated zero**.
*Rule: every new or moved test module in `bynk-emit`, `bynk-ide`, and `bynk-fmt`
uses the canonical two-line form.*

**B. `emit_diagnostics` has no test-range exclusion.** `bynk-emit` reads `4/6`; a
unit test asserting on an emitted import specifier (`"bynk.locale"` is a *module
name*, not a diagnostic code) is likely, not merely possible, to add a seventh
literal. *Guard before every commit touching `bynk-emit`/`bynk-check` source:*
`rg -o --no-filename '"bynk\.[a-zA-Z0-9_.]*"' <crate>/src | sort -u | wc -l` must still read
6 (`bynk-emit`) / 397 (`bynk-check`, naive) — `rg -o` prefixes each match with
its path when searching a directory, so `sort -u` without `--no-filename`
dedupes *(file, literal)* pairs, not literals; run against the tree as of this
correction, the command without that flag reads 439 for `bynk-check` (the flag
is required, not cosmetic — confirmed live: `bynk-emit`'s 6 happens to hold
either way only because every one of its literals sits in a single file,
`project.rs`).

**C. The C′ loophole in `ast_importers`.** `has_module_level_super_glob`
(`:1470`) is exact string equality on `use super::*;`. A split child that
reaches AST types via `use super::{Handler, TypeRef};` instead of a glob import
is caught by neither of the probe's two clauses — the count stays 5, the gate
passes, and the underlying AST dependency the probe exists to track is
unchanged. This is the same evasion #1259 hardened against, spelled with named
imports instead of a glob. **No slice in this track may split a file under
`bynk-emit/src/emitter{,/**}` or either `AST_IMPORTER_EXCEPTIONS` file** — that
subtree is out of scope until §10/S10's amendment lands, precisely because this
loophole makes "the count didn't move" insufficient evidence that a split there
is safe.

**Governing rule:** the gate is an all-or-nothing equality diff over 14 rows.
Any slice that moves a probe re-stamps (`cargo xtask greenfield-status --apply`)
in the same commit as the change that moved it, and no slice batches a
probe-moving change with a probe-invariant one — that would destroy delta
attribution the next reviewer needs.

## 4. Workstream 1 — Crate READMEs (S1–S3)

The template already exists in the tree; this workstream applies it rather than
inventing one. `bynk-syntax/README.md` is the canonical published-crate shape,
identical across all eleven published READMEs that currently exist: `# <crate>`,
three badges (crates.io, docs.rs, license), a bold one-line positioning sentence
plus a bulleted module inventory, `## Where it sits` (a layering diagram plus
consumers), `## Use` (a `toml` `[dependencies]` block, a `rust` snippet, a
docs.rs link), `## License`. `bynk-wasm/README.md` is the unpublished-crate
shape — license badge only, plus an explicit `> **Not a published crate.**`
callout — used for `bynk-testkit`, `tree-sitter-bynk`, `xtask`.

Two mechanical constraints bind any new `## Use` block: `scripts/bump-version.sh:77-83`
rewrites dependency pins with a `sed` pattern that requires the line to read
exactly `bynk-foo = "0.289"` at line start (no `{ version = ... }`, no
features), and `.github/workflows/stamp.yml:113-118` stages `bynk*/README.md` —
`xtask/README.md` falls outside that glob (harmless; it carries no version pin).
No manifest change is needed anywhere: no crate sets `readme = ...` today, and
Cargo auto-detects an adjacent `README.md`.

**S1 — the eight missing READMEs.** Five published crates
(`bynk-ts`, `bynk-ir`, `bynk-lower`, `bynk-project`, `bynk-driver`) on the
`bynk-syntax` template; three unpublished (`bynk-testkit`, `tree-sitter-bynk`,
`xtask`) on the `bynk-wasm` template. `bynk-driver/Cargo.toml` also gains
`keywords`/`categories` — the only publishable crate currently missing them.
`tree-sitter-bynk` is published to npm as well as crates.io-adjacent, so its
README also fixes an empty npm package page.

**S2 — kill the seven-copy diagram.** Replace the hand-maintained ASCII diagram
with one, generated from the real dependency graph rather than copied by hand,
plus a drift-guard test asserting each README's diagram matches the manifests —
the same pattern `bynkc/tests/decisions_index.rs` already runs for the ADR index
("completeness is mechanical and has drifted before"), in both the `test` and
`drift` CI jobs. Fix the root `README.md`'s "Repository layout" table in the
same slice — it lists five published crates against a real sixteen. Record the
`keep_in_sync` trend probe's before/after reading in the slice's own commit
message (239 today) — it is the only quantitative evidence that removing seven
hand-synced copies moved the P2 metric this slice's own rationale cites.

**S3 — a `crate_readmes` conformance test.** Every workspace member has a
README; every published one carries the three badges and a `## Use` block whose
pin matches the workspace version. This is what stops the workstream regressing
the way the diagram did. (Open design question 4, §7, settles the test's exact
contract.)

## 5. Workstream 2 — crate-local unit tests (S5–S7)

The reference pattern is already in the tree: `bynk-lower` reaches 66.6% test
density off a six-line helper at `bynk-lower/src/lib.rs:4069`,
`fn checked_program(source: &str) -> CheckedProgram`, chaining
lex → parse → resolve → check → certify. R11.1's "literal input, asserted
output" was achieved by a helper over an *already-correct* signature, not by
changing one — which reframes the triage question for every low-density crate:
does it lack an equivalent `source → value` entry helper because the signature
is wrong, or because nobody wrote one?

Triage by API shape, per P1/R11.1:

- **`bynk-syntax` (10.7%) — shape is right.** `tokenize` and
  `parse_with_warnings` are textbook total functions. Three files carry zero
  in-file tests: `parser/declarations.rs` (3269 lines), `parser/expressions.rs`
  (1762), `diagnostics.rs` (2453). Highest ROI and cheapest in the workspace.
  `diagnostics.rs`'s valuable tests are table invariants — every `REGISTRY` code
  unique, every `Explain` code present in `REGISTRY`, `category` total.
- **`bynk-check` (10.4%) — shape is right at the boundary.** `check(ResolvedCommons)
  -> Result<TypedCommons, _>` is already value-to-value; density is low because
  assertions were relocated to `bynkc`'s fixture corpus, and coarsely — 424
  fixtures use `expected_error.txt` (a category string) against 5 using
  `expected_diagnostics.txt` (location-attributed). Add a `checked(src)` /
  `errors(src)` helper mirroring `bynk-lower`'s. Pure leaves to start with:
  `context_checks::{ts_type_ref_display, type_ref_is_keyable, cache_ttl_millis}`,
  `calls::flatten_ident_chain`, `kernels::{is_orderable, is_numeric, is_keyable,
  is_query_op}`.
- **`bynk-emit` (11.8%) — the fix is the signature, not the test.** 26 functions
  currently take an `out: &mut String` sink (counted directly: a multiline scan
  for `fn <name>(...out: &mut String...)`, distinct names, across
  `bynk-emit/src`; a bare substring grep over-counts at 53, because 28 of those
  hits are doc comments narrating an *already-converted* function's old
  signature — "returns real `TsStmt`s (was `out: &mut String`)" — not a live
  parameter). This is **not** what either gated probe measures: `ts_writes`
  (809) counts `write!`/`writeln!`/`format!` call sites, not sink signatures,
  and `hoist_sinks` (gated, reads 0) tracks a different, already-eliminated
  needle, `stmts: &mut Vec<String>` (Tier B/T2.1's own sink, distinct from this
  one). No probe currently gates this count; §5's `ts_writes` citation was
  wrong to claim it did. **Do not bulk-add tests around a `&mut String`
  parameter** — it cements the shape R7 is in the process of removing. Add
  tests only to functions that already return `TsStmt`/`TsExpr`/`TsType`,
  extending the pattern the nine existing test modules in `emit.rs` already
  use.
- **`bynk-ir` (0.0%) — a real signature defect, not a testing gap.** ~40 data
  definitions need no test. But `block_uses_emit`
  (`bynk-ir/src/lib.rs:1792`) makes a pure data crate depend on **`bynk-check`**
  — the sharpest instance in the tree of P1's "low density may be a signature
  problem, not a discipline problem." Fixing this (moving the function to
  `bynk-lower`, which already owns Ast→Ir and depends on `bynk-check`, or
  parameterising it on `impl Fn(ExprId) -> Option<CalleeKind>`) is what makes
  `bynk-ir`'s 0.0% a *provably correct* reading rather than an untested crate —
  a better outcome than writing five tests around the wrong dependency.
- **`bynk-lsp` (35.7%) and `bynk-fmt` (15.6%) — split first, then test.** Both
  crates' low density traces to pure functions buried inside oversized files
  (`bynk-lsp`'s `cursor.rs` seam, `bynk-fmt`'s `to_string.rs`/`verify.rs` seams
  — see §6). The test opportunity is invisible until workstream 4 exposes it.
- **`bynkc` (0.0%) and `bynk-testkit` (0.0%) — correct as-is.** `bynkc` is a
  thin binary over 91 integration files / 281 tests / 888 fixtures that
  `test_density` structurally cannot see; `bynk-testkit` is 101 lines of disk
  adapters exercised by every consumer's own integration suite.

**`test_density` stays a trend probe, not gated** (open design question 3, §7).
`gated_disagreements` is exact string equality, so gating it would gate
byte-equality of a 19-crate percentage string — the maximal-churn version of the
probe, which is exactly what #999 Decision D declined for `wildcard_arms`. It is
also structurally coupled to workstream 4: every split adds `mod x;`/`use` lines
to the denominator, lowering density in every crate it touches, independent of
whether a single test was added or removed. If a gate is wanted later it needs
a new, zero/closure-shaped probe with a hand-argued floor table — absolute test
lines per crate, not a ratio, to avoid ratcheting against ordinary production
growth — not a flag flip on this one.

- **S5** — the pure-leaf tests above, plus the tests `bynk-lsp`'s and
  `bynk-fmt`'s S4 splits expose.
- **S6** — `bynk-emit` node-returning-function tests only.
- **S7** — the `bynk-ir` → `bynk-check` dependency-direction fix.

## 6. Workstream 4 — module decomposition (S4, S9; S10 deferred)

A file is *considered* at ~1,500 production lines. It is *split* only where
three tests all pass, at seams chosen by cohesion, never by line count:

- **T1 — named family.** The extracted set has a noun phrase from the domain
  vocabulary ("statement printing", "store annotations", "cross-context calls")
  — never "part 2".
- **T2 — import cut.** The child's `use` list is a strict subset of the
  parent's, and at least one parent import becomes unused as a result. *This is
  also the mechanical test for the C′ loophole (§3): a child that needs
  `use super::{…}` to reach the parent's imports has failed T2.*
- **T3 — test travel.** Every `#[test]` naming an extracted item moves with it
  and lands in exactly one child. A test asserting across two proposed children
  means the seam runs through a behaviour, not around one.
- **Hard veto — no orphan context.** A seam requiring a new `&mut Ctx` parameter
  that did not previously exist is a re-architecture, not a split.
- **Floor.** No child below ~600 production lines — a 200-line module adds file
  indirection without naming a family. **Carve-out:** a child below the floor
  is allowed when the extraction is itself the point — a pure, independently
  testable family the parent's size was hiding, per T1 (it still needs a real
  noun-phrase name, not a line-count justification). `bynk-lsp`'s `cursor.rs`
  (~200 lines of pure `&str -> Option<usize>`) and `bynk-fmt`'s `verify.rs`
  (~150) are both this case, not an exception to it — the whole point of
  splitting them out is making that small pure family visible and testable
  (workstream 2's own argument), which a 600-line floor applied uniformly
  would forbid.

**S4 — the safe set, ~66,700 lines (the sixteen sizes below sum to 66,713),
zero gated-probe exposure.**
`bynk-ts/src/printer.rs` (6703 lines) is the reference split — the cleanest
seams in the workspace and fully outside every probe's universe:
`printer/stmt.rs`, `printer/expr.rs`, `printer/ty.rs`, `printer/decl.rs`, with
`printer.rs` keeping `Printed`/`print`/`indent` and the one-shot API. Then, in
order: `bynk-lsp/src/lib.rs` (6535 — `state.rs`, `backend.rs`, `server.rs`,
`capabilities.rs`, `completion_triggers.rs`, and `cursor.rs`, ~200 lines of pure
`&str -> Option<usize>` currently invisible under 6,535 lines and the crate's
highest-value item-2 target), `bynk-check/src/checker.rs` (5301 — `types.rs`,
`callee.rs`, `program.rs` ⚠ carries a live `HashMap<Span` at `:700`, `bodies.rs`,
`contracts.rs`, `ctx.rs` ⚠ carries a live `HashMap<Span` at `:2144`, `tys.rs`;
`checker.rs` itself keeps `certify`/`check`, the crate's R11.1 boundary), plus
`context_checks.rs` (4335), `checker/expressions.rs` (4031), `checker/calls.rs`
(4017, 0 tests), `bynk-fmt/src/fmt.rs` (3811 — a `to_string.rs` seam of ~670
pure lines and a `verify.rs` seam of ~150), `bynk-check/src/project_model.rs`
(3448), `bynk-syntax/src/parser/declarations.rs` (3269, 0 tests),
`bynk-syntax/src/ast.rs` (3027), `bynk-ide/src/completion.rs` (2871),
`checker/kernels.rs` (2607, 0 tests), `bynk-syntax/src/diagnostics.rs` (2453),
`bynk-check/src/test_suites.rs` (2290), `bynk-ir/src/lib.rs` (1909), and
`bynk-lower/src/lib.rs` (10106 — the largest single target; T3 dominates here,
since 6,051 of its 10,106 lines are inline tests that must travel with the code
they exercise, and the `checked_program` prelude becomes a shared
`lower/testkit.rs` behind `#[cfg(test)]`).

**S9 — the carved set, splittable with specific items pinned.**
`bynk-emit/src/project.rs` (4502) may split, but `struct CompileOptions` stays
in `project.rs` (pinned by `options_sources`), the trailing test module stays in
`project.rs` in canonical form (pinned by `fs_below_driver`), and no child may
import the AST. `bynk-ide`'s five `std::fs` files may split with their trailing
`#[cfg(test)]` blocks kept intact and canonical.
`bynk-emit/src/emitter/{wrangler,toml_doc,secrets,contracts}.rs` do not split at
all in this track — a child would need a new `TS_WRITES_EXCLUDED_FILES` entry.

**Everything under `bynk-emit/src/emitter{,/**}` and both
`AST_IMPORTER_EXCEPTIONS` files is out of scope for this track** — see §10.

## 7. Open design questions

1. **Comment-provenance scope.** Does the superseding ADR (§8/S0) also drop
   greenfield rule citations (`R6.1`, `R11.2` — 328 of them), or keep them
   alongside ADR references because they cite a *living* spec
   (`bynk-greenfield-compiler.md`) rather than a retired one? **Leaning: keep
   them.** Dropping them roughly doubles the sweep (workstream 3, §9) and leaves
   several invariants with no citable source. Settle before S8 cuts.
2. **The `ast_importers` amendment's exact form.** §10 below is the argument;
   settling this track does not settle the amendment itself — that is S10's own,
   later settling review. What this track's settling pass must confirm is only
   that the amendment is *not* front-loaded here (i.e., that deferring the
   whole emitter subtree, rather than amending the probe now and proceeding, is
   the right call for this pass). See §10.
3. **Does `test_density` stay ungated?** §5's position: yes, for the reasons
   given there. Settle whether a future, separate, zero/closure-shaped probe
   (e.g. "no crate below `bynk-check` in the dependency graph may name
   `bynk_check::`" — the direction §5 actually found reversed, `bynk-ir`
   depending *up* on `bynk-check` via `block_uses_emit`, not the other way
   round) is
   worth building as part of S7, or left as a named follow-on.
4. **`crate_readmes`'s exact contract (S3).** Presence-only, or badges + `## Use`
   version-pin conformance + dependency-diagram conformance? Recommend the
   latter — a presence-only test cannot catch the drift that motivated §4 in
   the first place.

## 8. Front-loaded ADRs

**The comment-provenance policy** — the load-bearing, hard-to-reverse decision
this track's settling pass must identify, per `design/tracks/README.md` step 2.
ADR 0384 and ADR 0411 both record, verbatim: *"In-source doc comments citing
`<track>.md` … are left as historical references, not swept — the same explicit
decision every earlier track's own retirement made for its own doc. A citation
naming the doc that was true when the comment was written is a historical fact,
not a broken link."* This track's workstream 3 (§9) reverses that posture for
the specific classes named there — slice IDs, phase numbers, issue numbers,
citations to since-deleted track docs — while keeping ADR references and (per
open question 1) greenfield rule citations as surviving provenance. A PR
sweeping comments against a posture two standing ADRs explicitly reject, with
no ADR of its own, is arguing a settled decision in a review thread — precisely
what the ADR process exists to prevent. This ADR lands in **S0**, before any
comment sweep (S8).

**Not front-loaded here:** the `ast_importers` probe amendment. It is deferred
to its own slice (S10) with its own settling review — see §10.

## 9. Workstream 3 — comment rewrite (S0, S8)

Kept as surviving provenance: ADR references (1,216 comment-line hits across
the workspace) and, per this track's leaning on open question 1, greenfield
rule citations (`R6.1`, `R11.2` — 328 hits) — both cite documents that remain
live. Removed: slice IDs (`P6.1`, `Arc D`), phase numbers, issue numbers
(`#1141`), and citations of deleted `design/tracks/*.md` files — verified: 100
such citations across the workspace, every one dangling (the file it names no
longer exists on disk).

**The rule: keep the why, drop the when.** A comment earns its place if it
states an invariant, a constraint that cannot be derived from the code, or a
deliberate choice that would otherwise look like a bug. It does not earn its
place if its subject is the edit history of the comment or the call site.

**This is not a regex job.** Some comments' provenance tokens *are* the
explanation — `bynk-ts/src/program.rs:394-401` explains why `Raw` is a separate
variant from `Verbatim` entirely in terms of what the `verbatim_origins` and
`verbatim_sites` probes count and what "Arc C residue" means structurally. A
sweep that deletes the tokens there destroys the comment; it must instead
restate the fact in plain terms ("this variant is deliberately excluded from
the residue probes because it is permanent, not unconverted") rather than
delete the sentence. Budget the sweep for rewriting, not deleting.

Deletion is the right move for prose whose actual subject is edit history —
concrete examples found in the census: `bynk-lower/src/lib.rs:8-26` (nineteen
lines of crate-level doc retracting a claim an earlier version of the *same
comment* made), `bynk-lower/src/lib.rs:3086-3094` (eleven inline lines ending
"…as this comment used to say"), `bynk-lower/src/lib.rs:795-798`
("was `lower_handler_signature_ir` … the wrong sibling for a *service*
handler"), and `bynk-ir/src/lib.rs:1` (a crate-level `//!` opening with a slice
ID and issue number before saying what the crate is).

**These line anchors, and the §6 marker-hit counts below, are taken against the
pre-S4 tree — sequencing matters.** `bynk-lower/src/lib.rs`, `bynk-ir/src/lib.rs`
and (via `bynk-ts/src/printer.rs`) two of the census's five files are S4 split
targets; by the time S8 opens, `printer.rs` has already become
`printer/{stmt,expr,ty,decl}.rs` and its own marker hits will have scattered
across the children the same way its production lines do. **S4 runs before S8**
— the slice numbering already implies this, but it is worth saying outright
rather than leaving a later reader to infer it: treat every line anchor and
per-file hit count here as evidence for *sizing* S8, re-censused against
whatever tree exists once S4 lands, not as S8's literal worklist.

Comments that must be kept despite carrying markers, because no regex can
separate them from the defective class: `bynk-wasm/src/lib.rs:116-124` (states
a platform constraint — wasm panics trap, so `catch_unwind` doesn't help — you
cannot derive from the code), `bynk-ide/src/symbols.rs:29-33` (names a
*deliberate* inconsistency so a future reader doesn't "fix" it), and
`bynk-ts/src/program.rs:394-401` (above).

**Two hard carve-outs**, both because a gated probe reads the comment text
itself: `xtask/src/greenfield_status.rs` — its doc comments are the probes'
own living specification, several probes are self-referential about their own
search strings, and `span_keyed_maps` (gated, R2.4) deliberately counts
`HashMap<Span` occurrences *including comments*; and `Cargo.toml:59-62`, a
comment block that explicitly states `workspace_lints` reads the first line
naming the lint. Also carve out comments citing fuzz-found bugs as standing
guards.

- **S0** — the superseding ADR (§8) plus a written comment standard, added as a
  `contributing/` page in the Book. This is what makes S8 reviewable rather than
  taste-driven — there is currently no comment-writing standard anywhere in the
  repo (no `CONTRIBUTING.md`, nothing in the Book's `contributing/` pages, no
  lint, no CI grep).
- **S8** — the sweep, crate by crate, in census order:
  `bynk-lower/src/lib.rs` (302 marker hits), `bynk-emit/src/emitter/emit.rs`
  (376) and `emitter.rs` (244), `bynk-ts/src/printer.rs` (283),
  `bynk-ir/src/lib.rs` (234) — plus a rewrite of each affected crate's own
  crate-level `//!` header so it opens with what the crate *is*, provenance
  demoted below.

## 10. The deferred emitter subtree, and the `ast_importers` amendment (S10)

`emitter.rs` (5644 lines), `emitter/emit.rs` (7670), `emitter/lower.rs` (6321),
`emitter/workers.rs` (2149), `emitter/workers_entry.rs` (2492) — 24,276 lines —
are exactly the five files `ast_importers` counts to reach its gated floor of 5,
argued file-by-file at phase 6's own retirement
(`design/archive/retired-tracks.md`). `project/tests_emit.rs` (6377) and
`emitter/serialisation.rs` (3591) are the two `AST_IMPORTER_EXCEPTIONS`
entries; a split child of either would not inherit the exclusion, since the
list is exact-path, not prefix. Together, 34,244 lines — the single largest
item-4 opportunity in the workspace, and entirely out of this track's scope.

**Why deferred rather than included.** Splitting any of the five naively either
fails the gate outright, or — worse — passes it through the C′ loophole (§3): a
child reaching AST types via `use super::{Name}` rather than a glob import
moves no probe number while the underlying AST dependency the probe exists to
track is unchanged. The honest fix changes what the probe counts, and that
change deserves its own open review before any code moves — the same
discipline phase 6 and phase 7 each applied to their own probe-floor
arguments, not a decision folded into this track's settling pass as a side
effect of clearing the way for a split.

**The amendment's shape, recorded here so S10's own settling does not re-derive
it from nothing.** Growing `AST_IMPORTER_EXCEPTIONS` is closed by the probe's
own doc comment — *"This exclusion list does not grow to reach that floor."*
Phase 6's own retirement text says the floor is a **directory**, not an
integer: *"the floor is exactly `bynk-emit/src/emitter{,/**}`."* The 5 is an
artefact of how many files sat there on 19 August 2026, and the reason a
path-prefix rule was rejected at the time — it would have hidden `project.rs`'s
real open work — was cleared by P6.49; nothing outside `emitter{,/**}` and the
two named exceptions imports the AST today (verified). The proposed amended
row, using the same composite-string precedent `emit_diagnostics` and
`fs_below_driver` already set:

```
| ast_importers | yes | outside 0, vocabulary 43 |
```

`outside` = AST-importing files under `bynk-emit/src` that are neither
`emitter.rs` nor under `emitter/` — a true gated zero, verified today.
`vocabulary` = distinct `bynk_syntax::ast` names imported across
`bynk-emit/src` minus the exceptions — verified 43, and 43 under every
exclusion policy tried (with/without comment-stripping, with/without
`#[cfg(test)]`-range exclusion), so the choice is not load-bearing for the
committed value. This is a *finer* ratchet than the file count: converting one
name family drops `vocabulary` even when no whole file clears, which the old
probe was structurally blind to; and it kills the C′ loophole outright, since
relocating a name to a child via named imports leaves the union unchanged.

**S10 opens once S0–S9 land**, as its own slice with its own settling review —
not automatically; the amendment needs to be argued in the open before any
emitter-subtree file is split, per the ordering stated in §6.

## 11. Found, not fixed — named here, filed and resolved outside this track

Discovered while grounding this track's probe and manifest claims; none is a
consequence of the migration this track cleans up after, and none should ride
one of this track's own slices:

- **`bynk-project` is missing from both release workflows' hardcoded publish
  lists** (`.github/workflows/release.yml:334`,
  `release-bootstrap.yml:121`), despite being publishable and a
  version-pinned `[dependencies]` entry of `bynk-emit` and `bynk-ide`, both of
  which *are* on the list. The last release run predates `bynk-project`'s
  extraction (30 July 2026 vs. 7 August 2026), so this has not been exercised
  since phase 4 and will break the next release regardless of this track's
  timeline. Filed:
  [#1559](https://github.com/accuser/bynk/issues/1559).
- **`bynk-testkit` is a versioned dev-dependency of three published crates**
  (`bynk-lsp`, `bynk`, `bynkc`) while itself `publish = false` — confirmed
  live that neither release workflow passes `cargo publish` `--no-verify`, so
  publish verification builds the dev-dependency graph too; unverified
  against a real dry-run whether that actually fails. Filed:
  [#1560](https://github.com/accuser/bynk/issues/1560).
- **Three entries in the probe harness's own exception lists name files the
  P7.12 crate carve deleted** — `NAMED_FS_EXCEPTIONS` names
  `bynk-emit/project/discovery.rs` twice and `project/paths.rs` once;
  `TS_WRITES_EXCLUDED_FILES` names `emitter/source_map.rs`. None exists;
  `bynk-emit/src/project/` today holds only `diagnostics.rs`,
  `schema_registry.rs`, `tests_emit.rs`. `fs_below_driver`'s "0 named floor" for
  `bynk-emit` is therefore currently vacuous — it reads zero because the
  exception targets are gone, not because the residue they named was cleared.
  Filed: [#1561](https://github.com/accuser/bynk/issues/1561).
- **`design/tracks/README.md` notes `agent-capability-encapsulation.md` is a
  committed Draft in neither the active-tracks table nor
  `retired-tracks.md`** — needs a spine issue or a retirement, unrelated to this
  track's theme.
