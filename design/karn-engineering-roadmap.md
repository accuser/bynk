# Karn engineering roadmap — CI/CD + compiler internal quality

A forward plan for the non-language engineering work: the build/test/release
pipeline, and the `karnc` crate's internal-quality refactor backlog. A design
reference, not a per-increment proposal; concrete slices become
`proposals/vX.Y-*.md` when scheduled.

> _Consolidates the former `karn-cicd-roadmap.md` and
> `karn-refactor-proposal-queue.md` (both June 2026)._ Part A is the pipeline;
> Part B is compiler paydown. The refactor backlog (Part B) was captured from a
> June 2026 code review; several structural splits have since landed under the
> refactor track (ADR 0060) — statuses below are refreshed, but re-verify against
> the current tree before scheduling.

---

# Part A — CI/CD pipeline

**Tiers 1–3 are implemented; Tier 4 is gated on external credentials, not
engineering.**

## A.1 Current state (after the Tier 1–3 pass)

**CI** (`.github/workflows/ci.yml`, on push / PR / weekly Mon 07:00 UTC) — a
`changes` detection job (`dorny/paths-filter`) gates the rest so a PR only pays
for what it touched; a single `ci-green` aggregator is the one required check:

- `changes` — emits `rust` / `docs` / `extension` / `grammar` booleans plus an
  `all` escape hatch. `all` is true on any non-PR event or when a *global* file
  changed (a workflow, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`). The
  cross-component edges are encoded in the gates: `test` also runs on `docs`
  (karnc's suite reads `docs/src/**`), `docs` also runs on `rust` (the book
  renders through the Rust mdBook preprocessors), and `extension-tests` also runs
  on `rust` (it builds `karn-lsp` from source).
- `ci-green` — `needs:` every job, `if: always()`; red only if a needed job
  *failed* or was *cancelled* (a skipped job is a pass). Branch protection
  requires this one check, which makes the path-gating safe and decouples the
  ruleset from individual job names.
- `fmt`, `clippy` — gated on `rust`.
- `test` — `cargo test --workspace --locked` with `KARN_REQUIRE_TSC=1`, matrixed
  across ubuntu / macOS / windows (`fail-fast: false`); `typescript@5` pinned.
- `msrv` — `cargo check --workspace --locked` on the declared `rust-version`
  (1.85).
- `docs` — mdBook via `taiki-e/install-action` (prebuilt) + linkcheck.
- `extension` — `npm ci` → `tsc --noEmit` → esbuild bundle →
  `scripts/check-bundle.mjs` → `vsce package`.
- `grammar` — `tree-sitter generate` + `tree-sitter test` (the corpus).
- `audit` — `cargo audit` (RustSec); `deny` — `cargo-deny`;
  `dependency-review` on PRs.

**Release** (`release.yml`): two-phase, five-target binaries + GitHub Release;
manual-dispatch publish to crates.io + npm. `SHA256SUMS` over the archives,
signed build provenance (`actions/attest-build-provenance`), npm `--provenance`,
and crates.io OIDC Trusted Publishing. **Pages** (`pages.yml`): mdBook → GitHub
Pages on `main`. **Dependabot**: cargo, github-actions, npm.

## A.2 Done — Tiers 1–3

- **Tier 1 (proven holes)** — CI for the VS Code extension (with the
  `check-bundle.mjs` smoke gate); the tree-sitter grammar corpus job; the
  multi-OS test matrix with `.gitattributes` LF normalisation.
- **Tier 2 (release integrity & supply chain)** — checksums + build provenance;
  crates.io OIDC Trusted Publishing; `cargo-deny`; PR dependency-review.
- **Tier 3 (reproducibility hygiene)** — pinned `typescript@5`; `--locked` on CI
  `test`; declared MSRV 1.85 *with* a CI leg that builds on it; prebuilt mdBook.

## A.3 Remaining — Tier 4 (distribution polish)

> **Public-flip note.** The repo went public at v0.43.0; the private-only gates
> self-healed as designed (the `ubuntu-24.04-arm` release leg is live;
> `dependency-review` runs on PRs). Tier 4 has no readiness blocker — the two
> unfinished items are gated on external credentials.

- **Extension + grammar release automation** — per-platform VSIXs bundling
  `karnc-lsp`, published to the VS Code Marketplace + Open VSX (the tooling
  roadmap's B-0/B-2 from the CI angle). *Needs marketplace tokens.*
- **Binary signing / notarisation** — macOS notarisation + Windows signing for
  the downloaded binaries. *Needs certificates.*
- **Supply-chain posture** ✅ **Done** — OpenSSF Scorecard (`scorecard.yml`) and
  all actions SHA-pinned across the workflows.

See [`karn-tooling-roadmap.md`](karn-tooling-roadmap.md) §7.3 — Tier 4's
extension publishing and the server-provisioning work are the same effort from
two angles.

---

# Part B — Compiler internal-quality refactor backlog

Structural and maintainability changes only — no observable behaviour change, no
language-surface change. Captured from a June 2026 code review whose headline was
*structurally healthy, idiomatic, disciplined error handling and CI — the work
below is incremental paydown, not remediation*.

## B.1 Structural decomposition

1. **Split `project.rs` into submodules.** ✅ **Done.** The crate now carries
   `project/{paths,discovery,consistency,graph,symbols,diagnostics,tests_emit,
   validate}.rs` exactly as proposed; `project.rs` dropped from ~7.9k to ~3.4k
   lines (ADR 0060, the refactor track v0.29.x).
2. **Decompose `compile_project_pipeline`.** Re-verify: the project split likely
   moved much of this; confirm the back-half (per-unit symbol composition, the
   `uses`/`consumes` merge loops, emission dispatch) is now extracted.
3. **Decompose the next two god functions** — `lower_expr` (`emitter.rs`) and
   `check_v0_5_declarations`. Both have natural per-arm extraction points;
   `lower_expr`'s larger match arms should delegate to per-`ExprKind` helpers.
4. **Give `checker.rs` navigation, and tame `Ctx`.** ◑ **Partly done** —
   `checker/{calls,refinements,expressions,kernels}.rs` now exist. Remaining:
   group the ~6 capability-related `Ctx` fields into a `CapabilityCtx`
   sub-struct, and finish banner/navigation across the remaining checker bulk.

## B.2 API & internal modelling

5. **Collapse the `compile_project*` API into an options struct.** Several public
   variants over orthogonal axes (`target`, `platform`, `paths`) — replace with
   `CompileOptions { … }`, removing the `_full` doubling and the `Mode`-driven
   `unreachable!()` guards. Touches `lib.rs` re-exports and every caller
   (`main.rs`, `karn-lsp`).
6. **Introduce a `UnitInfo` aggregate to kill the parallel maps.** Several
   `HashMap<String, _>` keyed on the same unit name are looked up with
   `.get(name).unwrap()` repeatedly; one `HashMap<String, UnitInfo>` makes the
   shared-keyset invariant structural. *Pairs with item 2.*

## B.3 Consolidation / DRY

7. **Eliminate the second TypeScript emitter.** The test-emission helpers (now in
   `project/tests_emit.rs`) carry their own `escape_ts_string`/`ts_type_ref_emit`
   — a parallel TS generator that risks drift against the real `emitter/`.
   Consolidate.
8. **Centralise the stringly-typed built-in names.** Built-in type/method
   literals (`"Json"`, `"List"`, `"Map"`, `"Float"`, `"of"`, `"foldEff"`, …) are
   scattered as bare string comparisons across ~13 checker sites; gather into
   `mod builtin_names { pub const … }` or an enum.
9. ~~**Add a `CodeWriter` / `wl!` indentation helper.**~~ ❌ **Shelved.**
   Inspecting the emitter contradicted the proposal's premise (indentation is
   *not* centrally threaded — most sites hardcode leading spaces as literal
   content). See `archive/v0.29.6-refactor-codewriter.md`.

## B.4 Testing (de-risks the splits above)

10. **Decide `insta`: adopt or drop.** A declared dev-dependency that is entirely
    unused; either adopt it for the emitter's TS snapshots or remove the dep.
11. **Add seam-level unit tests to the big files.** Pure helpers
    (`canonicalise_cycle`, `normalize_rel`, `unit_path_matches`, the cycle DFS)
    are only exercised transitively; direct tests make further decomposition
    safer.

## B.5 Lower priority / latent

12. **Resolver declaration-cloning.** The resolver clones whole declarations into
    symbol tables; `Rc<_>`/arena interning or storing indices would remove the
    cost. *Do when a scale signal appears.*
13. **Version-marker comment convention.** 300+ `v0.NN (ADR NNNN)` markers — net
    positive, but lead with the *what*, trail with *(since vX / ADR Y)*, and
    prune bare version tags once a feature is baseline. *Editorial; apply
    opportunistically.*

## B.6 Suggested sequence

Seam-level unit tests (11) first for any helper about to move. The big
`project.rs` split (1) has landed; the pipeline decomposition (2) + `UnitInfo`
(6) and the `compile_project*` API collapse (5) are the natural next cluster.
Then `lower_expr` (3) and the duplicate-TS-emitter removal (7). `checker.rs`/`Ctx`
(4) and built-in-name centralisation (8) slot into any calmer increment. `insta`
(10) is a quick standalone decision; resolver cloning (12) waits for a scale
signal; the comment convention (13) is opportunistic.
