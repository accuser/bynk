# Bynk compiler — refactor proposal queue

The concrete backlog of *internal-quality* refactors for the `bynkc` crate, ordered by recommended
sequence. A planning reference (complements `bynk-tooling-proposal-queue.md`, which tracks *feature*
work); each item becomes a `design/proposals/vX.Y-*.md` when scheduled. These are structural and
maintainability changes only — no observable behaviour change, no language-surface change. Sizes are
rough; "gated" notes a prerequisite.

These items were captured from a code-quality review of the `bynkc` binary (June 2026). The review's
headline: structurally healthy, idiomatic, disciplined error handling and CI — the work below is
comfortably incremental paydown, not remediation. Concentration is the theme: four files hold ~26k of
the crate's ~37k lines.

---

## 1. Structural decomposition (the big splits)

1. **Split `project.rs` (7,912 lines) into submodules.** *The single highest-value refactor.* It
   mixes at least six unrelated jobs: path/convention resolution, file discovery, structural-
   consistency checks, dependency-cycle detection, symbol-table construction, error attribution, and
   test/integration emission. Natural seams:
   `project/{paths,discovery,consistency,graph,symbols,diagnostics,tests_emit,validate}.rs`. The
   existing `emitter/` submodule split is the template — the team has done this cleanly before.
   *Mechanical; lowers cognitive load before anything else can be done safely.*
2. **Decompose `compile_project_pipeline` (~1,800 lines).** Phase markers (`// -- N. --`) cover the
   front half; the back half (per-unit symbol composition, the `uses`/`consumes` merge loops, emission
   dispatch, output assembly) is unmarked and deeply nested. Each phase is a candidate function taking
   explicit inputs and returning its products; the existing sinks (`ErrorSink`/`RefSink`/`HintSink`)
   already thread cleanly enough to make extraction mechanical. *Meaty; do alongside or just after item
   1.*
3. **Decompose the next two god functions** — `lower_expr` (~600 lines, `emitter.rs`) and
   `check_v0_5_declarations` (~500 lines, `project.rs`). Both have natural per-arm extraction points;
   `lower_expr`'s larger match arms (`Call`, `BinOp`, `Match`, string-kernel paths) should delegate to
   per-`ExprKind` helpers. *Medium; independent of 1–2.*
4. **Give `checker.rs` (6,694 lines) navigation, and tame `Ctx`.** No section banners across 119
   functions. `Ctx` is a ~20-field god-context with six capability-related fields alone
   (`capabilities`, `declared_capabilities`, `given_remaining`, `given_used`, `given_entries`,
   `given_anchor`). Split into submodules (`checker/{calls,refinements,match}.rs`) and group the
   capability fields into a `CapabilityCtx` sub-struct. *Meaty; sliceable (banner-and-split first,
   sub-struct second).*

## 2. API & internal modelling

5. **Collapse the `compile_project*` API into an options struct.** Six public variants (`_full`,
   `_with_target`, `_with_platform`, `_with_split_paths`, `_with_split_paths_full`, plain) over
   orthogonal axes — a combinatorial smell that grows multiplicatively. Replace with
   `CompileOptions { target, platform, paths }` and let callers `.flatten()` for the non-attributed
   error shape rather than doubling each entry point with `_full`. This also removes the two
   `unreachable!()` guards that the dynamically-passed `Mode` parameter currently forces (the
   `PipelineResult` sum type is a poor man's two return types). *Small–medium; touches `lib.rs`
   re-exports and every caller (`main.rs`, `bynk-lsp`).*
6. **Introduce a `UnitInfo` aggregate to kill the parallel maps.** Several `HashMap<String, _>` keyed
   on the same unit name (`kinds`, `unit_tables`, `exports_visibility`, `unit_uses`, `unit_consumes`)
   are looked up with `.get(name).unwrap()` ~10+ times across the pipeline. The "all maps share one
   keyset" invariant lives only in the programmer's head and the `.expect` messages. One record
   (`HashMap<String, UnitInfo>`) removes the unwraps and makes the invariant structural. *Medium;
   pairs with item 2.*

## 3. Consolidation / DRY

7. **Eliminate the second TypeScript emitter.** `project.rs`'s test-emission helpers carry their own
   `escape_ts_string`, `ts_type_ref_emit`, `sanitise_*` — a parallel TS generator living outside
   `emitter/`, which risks escaping/formatting drift against the real emitter. Consolidate into the
   emitter module. *Small–medium; do after item 1 surfaces `project/tests_emit.rs` as the seam.*
8. **Centralise the stringly-typed built-in names.** Built-in type/method literals (`"Json"`,
   `"List"`, `"Map"`, `"Int"`, `"Float"`, `"HttpResult"`, `"of"`, `"unsafe"`, `"raw"`, `"foldEff"`)
   are scattered as bare string comparisons across ~13 checker sites; a rename or typo is silent and
   they are hard to enumerate. Gather into `mod builtin_names { pub const … }` or an enum. *Small.*
9. **Add a `CodeWriter` / `wl!` indentation helper.** The emitter writes ~250 `writeln!(out, …)
   .unwrap()` into a `String` and threads an `INDENT_STEP: usize` by hand. A thin wrapper with
   indent tracking retires the infallible-`unwrap` noise and centralises indentation. *Small; ideally
   before item 3 so `lower_expr` extraction lands on the new writer.*

## 4. Testing (de-risks the splits above)

10. **Decide `insta`: adopt or drop.** It is a declared dev-dependency but entirely unused (no
    `assert_snapshot!`, no `.snap` files); a hand-rolled golden-fixture harness does the equivalent.
    Either adopt it for the emitter's TS output (snapshot review is cheaper than the bespoke differ)
    or remove the dep. *Small.*
11. **Add seam-level unit tests to the big files.** `checker.rs`, `project.rs`, and `emitter.rs` have
    ~4 `#[test]` each; coverage is strong but almost entirely end-to-end. Pure helpers
    (`canonicalise_cycle`, `normalize_rel`, `unit_path_matches`, `ts_type_ref_emit`, the cycle DFS) are
    only exercised transitively. Adding direct tests on these *before* items 1–3 makes the
    decompositions far safer. *Medium; partly a prerequisite for the section-1 work.*

## 5. Lower priority / latent

12. **Resolver declaration-cloning.** The resolver clones whole function/type declarations into symbol
    tables (`f.clone()`, `t.clone()` in `build_unit_table` and friends). Fine at current scale; a
    latent cost that `Rc<_>`/arena interning or storing indices would remove. *Do when a scale signal
    appears — premature otherwise.*
13. **Version-marker comment convention.** 300+ `v0.NN (ADR NNNN)` markers in the front-end alone —
    net positive (they tie code to decision records), but many comments lead with the version tag over
    the behaviour, which front-loads provenance over intent. Adopt a convention: lead with the *what*,
    trail with *(since vX / ADR Y)*, and prune bare "vX added this" tags once a feature is baseline and
    ubiquitous. *Editorial; apply opportunistically during the other refactors rather than as a pass.*

---

## Suggested sequence

**Seam-level unit tests (item 11)** first, at least for the helpers about to move — they are the
safety net for everything else. Then **split `project.rs` (1)** and **decompose its pipeline (2)** with
the **`UnitInfo` aggregate (6)** folded in, since they touch the same code. **Collapse the
`compile_project*` API (5)** naturally follows the pipeline work. Land the **`CodeWriter` helper (9)**
before **decomposing `lower_expr` (3)** and **eliminating the duplicate TS emitter (7)** so both land
on the new writer. **`checker.rs` navigation + `Ctx` (4)** and **built-in name centralisation (8)** are
independent and can slot into any calmer increment. **`insta` (10)** is a quick standalone decision.
**Resolver cloning (12)** waits for a scale signal; the **version-marker convention (13)** is applied
opportunistically throughout rather than scheduled.
