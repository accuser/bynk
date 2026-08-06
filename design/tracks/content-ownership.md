# Project content ownership — `bynk-lsp` becomes the sole reader of `.bynk` source content

- **Status:** All slices shipped (#1089, #1092, #1094, #1096, #1098, #1102);
  this doc retires next (§4, §7, §8). This doc's
  first pass merged still carrying every §3 question open (PR #1087, merged
  as ready-for-review without the review actually testing that assertion —
  exactly the failure mode `design/tracks/README.md`'s lifecycle step 2 warns
  about, so the doc's real phase stayed **Settling** past that merge). A
  re-settling pass closed §3.1–§3.5 for real and front-loaded the three §5
  ADRs (0322–0324). Slice 0 (cut as #1089, citing ADR 0322) then found, under
  implementation, that §3.1's `ProjectDirs`/`resolve_dirs` design wasn't
  needed — `bynk_ide::discover_files` already closes the gap — and that a
  scaffolding-only slice 0 doesn't independently ship in this repository
  (`cargo clippy -D warnings`'s dead-code gate), so the original slices 0 and
  1 merged into one shipped slice ([ADR 0325](../decisions/0325-content-ownership-seam-simplification.md),
  superseding ADR 0322); see §3.1/§4/§7 below, updated in place rather than
  rewritten. Slice 1 (renumbered — `symbols.rs`'s cross-file lookups, #1092)
  shipped next, and retired `Backend::project_files` entirely once it was
  `project_content`'s last remaining caller migration: `fs_below_driver`
  reaches 0 for `bynk-ide`. Slice 2 (`AnalysisRoots::lower()`'s `bynk.toml`
  read, #1094) shipped next — §3.5's settled fix, unchanged by slice 0's
  correction (it never depended on `ProjectDirs`). Slice 3 (`bynk-testkit`,
  #1096) shipped next, proved on three of its four planned call-site
  groups — `bynk-ide`'s own inline tests turned out to be a fourth group
  this crate structurally cannot serve (§3.3's correction) — and found a
  real bug (canonicalised sources-map keys breaking a project-consistency
  check) before it could reach the full migration. Slice 4 (#1098) is
  underway: sub-slice 1 migrated `bynk-ide`'s own 18 inline-test sites via an
  in-crate `testkit` module — a second real correction found here too (an
  inline `#[cfg(test)] mod` block, not a separate file, or `fs_below_driver`
  regresses; see §4). Sub-slice 2 migrated all 25 `bynk-lsp/tests` sites via
  `bynk-testkit` (21 bare `diagnose_project` + 4 `diagnose_project_with` —
  the latter including `project_model.rs`'s shared `rel_files` helper, whose
  single conversion fixed 8 further tests indirectly). Sub-slice 3 migrated
  the remaining 78 sites — 27 `diagnose_project` in `bynkc/tests`, 50
  `CompileOptions::single`/`::split` in `bynkc/tests`/`bynk/tests`, and
  `bynk-emit/src/project.rs`'s own 1 site — completing the migration. No
  separate §3.4 CI guard was built: slice 5's own fallback deletion turned
  out to already be the loud-failure mechanism the guard was meant to add
  (§3.4's final correction). Slice 5 (#1102) shipped last, deleting
  `read_source`'s content-reading fallback — and, driven entirely by what
  that deletion surfaced under implementation (no test suite green until
  each was found and fixed), three corrections beyond the doc's own
  "narrow" scope:
  (a) two production `bynk-lsp` paths — `run_project_diagnostics` (the
  published diagnostics round) and `type_receiver`'s slow-path
  re-analysis — built their overlay from open buffers only, silently
  depending on the fallback to read every closed file; both now sweep full
  content first, the same way `Backend::project_content` already did;
  (b) `AnalysisRoots::lower`/`discover_files`'s own `bynk.toml` read had the
  identical dependency for *every* manifest-backed caller, not just an
  unsaved-edit one — R2.3 forbids fixing this inside `bynk-ide` itself, so
  the real disk read moved out to the callers already above the driver
  boundary (`bynk-lsp`'s content sweep, `bynk-testkit`'s test seam),
  `discover_files` gaining an explicit overlay parameter to carry it;
  (c) an adapter's `.binding.ts` module — a distinct concern from `.bynk`
  project sources, since its *path* is only known once its declaring
  adapter is parsed, so no discovery walk can pre-populate it — keeps a
  narrow, dedicated disk-read helper (`read_adapter_binding`), never folded
  into the general `.bynk`-content fallback that was actually this track's
  target. `fs_below_driver` now reads 3 for `bynk-emit` (discovery.rs's
  enumeration walk and adapter-binding read, plus `paths.rs`'s own
  `bynk.toml` read for the same reason as (b) above, all above what a caller
  can pre-supply), 0 for `bynk-ide`; see §8. Spine issue
  [#1086](https://github.com/accuser/bynk/issues/1086) stays open until this
  doc's retirement PR closes it.
- **Realises:** R2.3 (`../bynk-greenfield-compiler.md`, its rules table at
  line 2515) — *"no ambient filesystem or global state; `Sources` is
  constructed once, at the process edge, and is the compiler's only view of
  file contents"* — specifically the `bynk-ide` row of `fs_below_driver`, the
  probe that checks it (`../greenfield-status.md:12`), open since T0.7
  (#1006/#1012).
- **Posture:** Feature track per
  [ADR 0076](../decisions/0076-feature-track-posture.md). Qualifies on two
  axes (§2): it is multi-increment (production-code seam, then a ~125-site
  test-harness migration that depends on it), and its surface is not yet
  settled (the new `bynk-ide`-exposed seam type, whether directory
  *enumeration* is in scope alongside content, and the test-harness
  replacement convention). Not a security/safety boundary — §6.
- **The unusual starting position — five investigations, one shared
  bottleneck.** #1006 first flagged the pattern; #1047 counted it (T0.7
  residue, 6 files); #1077 and #1079 each scoped one half (the CLI path,
  `bynk-ide`'s cross-file reads) as independently closeable; #1081 and #1084
  each investigated closing it and found the true scope larger than the issue
  text in front of them. #1084's finding is why this doc exists: `bynk-emit`'s
  `discovery.rs` fallback cannot be deleted by fixing `completion.rs`/
  `symbols.rs` alone, because ~125 test call sites across `bynk-ide`,
  `bynk-lsp/tests`, and `bynkc/tests` depend on that same fallback existing.
  #1077's own remaining scope (the CLI-path manifest read) was closed directly
  (`343b2482`, 5 August 2026) and the issue closed as completed; what's left —
  the content fallback itself, and everything that depends on it — is this
  track's whole scope, filed against #1079.

## 1. The theme

Three code paths read `.bynk` source content straight from disk, bypassing
every overlay the driver or the LSP has already built:

1. `bynk-emit/src/project/discovery.rs`'s `read_source` — overlay-first, but
   falls back to `fs::read_to_string` for anything the caller's overlay
   doesn't cover. This is the sanctioned seam's *fallback path*, not an ad hoc
   read, but the fallback is what keeps `fs_below_driver` non-zero for
   `bynk-emit`'s two flagged files: `discovery.rs` itself, and `project.rs`
   (`use std::fs;` at line 24) — a production-scope import that exists only to
   serve `discovery.rs` through `use super::*;`, and so becomes removable only
   once `discovery.rs` has no live `fs::` call left (§4, slice 6).
2. `bynk-ide/src/completion.rs`'s `cached_project_unit` — a per-file parse
   cache keyed by mtime+length, reading `std::fs::metadata`/`read_to_string`
   directly, consulted by every `for_each_unit` call (11 sites in
   `completion.rs`, 2 in `signature_help.rs`).
3. `bynk-ide/src/symbols.rs`'s `find_declaration_cross_file`/
   `describe_symbol_cross_file` — one `std::fs::read_to_string` each, in a
   loop over every candidate project file.

`bynk-lsp` already has half of what it needs: an open-buffer overlay
(`HashMap<PathBuf, String>`, built in `run_project_diagnostics`,
`bynk-lsp/src/lib.rs:596`) it passes to `diagnose_project_with`. It doesn't
cover files nobody has open, so `completion.rs`/`symbols.rs`'s direct disk
reads are how those get read at all — the overlay narrows the problem, it
doesn't close it. The end state this track builds toward: `bynk-lsp` adds the
other half (a full on-disk sweep for the files the overlay doesn't cover),
becomes the sole owner of every project file's content, and hands `bynk-ide`
pre-read `(path, content)` pairs everywhere it currently hands bare paths for
`bynk-ide` to read itself. `bynk-emit`'s fallback then has no caller left that
needs it and gets deleted. `fs_below_driver` goes from 4
(`bynk-emit=2, bynk-ide=2`, confirmed current via `cargo xtask
greenfield-status`) to 0.

A real behaviour fix rides along, not just an architecture cleanup: today, an
unsaved edit in file A is invisible to a completion, hover, or go-to-
declaration triggered from file B, because `cached_project_unit`/the
cross-file lookups read A's last-saved disk content, not the live editor
buffer. Closing the architectural gap closes this staleness as a side effect —
structurally the same shape as [ADR 0202](../decisions/0202-the-freshness-contract.md)
(the LSP's *position*-freshness contract: never answer from a stale analysis
snapshot), but here for file *content* rather than an analysis round.

## 2. Why a track (the ADR 0076 trigger)

- **Multi-increment.** The production-code half (§4, slices 0–2: `bynk-lsp`'s
  sweep, migrating ~13 call sites) and the test-harness half
  (§4, slices 3–5: replacing the `diagnose_project(&root, &HashMap::new())`
  convention across ~120 call sites, then deleting the fallback) are each too
  large for one delete-on-merge proposal — and the second cannot start until
  the first has shipped something for tests to call instead.
- **Surface not settled at the doc's first merge — is now.** Five genuinely
  open questions (§3), all closed by this re-settling pass: the shape of the
  new `bynk-ide`-exposed seam (§3.1, settled: `ProjectDirs`/`resolve_dirs`);
  whether R2.3's "no ambient filesystem" also bans directory *enumeration*,
  not just content reads (§3.2, settled narrow); what replaces a ~125-site
  test convention without turning every test file into boilerplate (§3.3,
  settled: a new `bynk-testkit` crate, exact helper shape deferred to slice
  4); migration order and mixed-state safety (§3.4, settled incremental with
  a structural CI guard); and `AnalysisRoots::lower()`'s own `bynk.toml` read
  (§3.5, settled — falls out of §3.1). §3.1–§3.3 are the three §5 front-loads,
  landed as ADRs via `design/pending/settle-content-ownership-track.md`.
- **Security/safety boundary — no.** This is an architectural correctness and
  testability property, not an attacker-facing boundary; §6 explains why.

## 3. Open design questions

### 3.1 The `bynk-ide`-exposed seam shape — SETTLED, then SUPERSEDED under implementation (slice 0, shipped)

**Originally settled, during this doc's re-settling pass.** `bynk-ide` would
gain a new public type and method, `ProjectDirs`/`AnalysisRoots::resolve_dirs`
— see [ADR 0322](../decisions/0322-content-ownership-seam-type.md) for the
design as settled. Front-loaded as §5's first ADR.

**Corrected under implementation (one real correction, the idempotency-capability
track's own precedent for this: a settled design not surviving contact with
the code, found only once building it).** `bynk-ide` already exposes `pub fn
discover_files(roots: &AnalysisRoots) -> Vec<PathBuf>` (`bynk-ide/src/lib.rs:234`),
which resolves `include`/`exclude` and enumerates a project's `.bynk` files
with no `bynk-emit` type crossing into `bynk-lsp` — and was already in
production use, by `Backend::project_files` (`bynk-lsp/src/lib.rs`), for
exactly the enumeration `ProjectDirs` was designed to enable. `bynk-lsp`
never needed *directories* to do its own walk; it needed *files*, and the
function that already gives it that list was sitting unused for this
purpose. No `ProjectDirs`/`resolve_dirs` were built. See
[the superseding ADR](../pending/content-ownership-seam-simplification.md)
(pre-stamp) for the full account, including why a scaffolding-only slice 0
("land the map, no caller yet") was not independently shippable in this
repository (`cargo clippy -D warnings`'s dead-code gate) and merged with
slice 1 into one shipped slice — §4 reflects the merged decomposition.

`bynk-lsp/Cargo.toml` still gains the dependency-exclusion comment
`bynk-ide/src/lib.rs:31`'s doc comment already (until now, incorrectly)
claimed existed — that part of the original design was right and ships
unchanged.

### 3.2 Does enumeration count as "ambient filesystem" under R2.3? — SETTLED, narrow

**Decision.** Narrow reading. `discover_bynk_files`
(`bynk-emit/src/project/discovery.rs:286`, a bare `fs::read_dir` tree walk
with no overlay parameter at all — confirmed structurally distinct from
`read_source`, which has one) stays below the driver. Only `read_source`'s
content-reading fallback branch (`discovery.rs:28-40`) is this track's scope.
`fs_below_driver`'s probe gets amended, as a named follow-on (not silently
left), to stop flagging pure-enumeration functions once slice 6 lands — so
`bynk-emit` reads 2 (both `discovery.rs` and `project.rs`'s `use std::fs;`
serving it, per §1) until the probe amendment, then a defined non-zero-by-design
floor, not 0.

**Slice 5 correction (found only under implementation): a second, narrower
carve-out.** `read_source` served two genuinely different reads —
`.bynk` project source content (this track's actual target) and an
adapter's `.binding.ts` module content (`project.rs`'s adapter-binding
resolution). The latter's *path* is only known once its declaring adapter
has been parsed (`adapter … { binding: "…" }`), so no discovery walk —
`bynk-testkit`, `bynk-driver::discovery`, or any future replacement — can
enumerate it ahead of time the way `.bynk` files are enumerated by
extension. Deleting the fallback outright broke every real adapter-binding
fixture (`bynk.adapter.no_binding` on every one). Fixed by giving this read
its own dedicated helper (`read_adapter_binding`, still in `discovery.rs`,
still overlay-first) that keeps a disk-read fallback — a deliberate,
narrow, permanent carve-out, not a straggler this track failed to close.
Likewise, `try_read_project_paths`/`read_project_paths` (the plain,
no-overlay manifest readers `bynk dev` and `bynk-driver::discovery` call
directly) turned out to depend on the same fallback for `bynk.toml` itself;
fixed with a real disk read in `paths.rs`, for the identical "not every
caller can pre-supply this" reason. `bynk-emit` reads **3** as of this
slice, not 2 — see §8.

**Why not broad.** R2.3's own wording — "the compiler's only view of file
*contents*" — is content-scoped on its face; broad is an extension, not the
literal rule. The candidate mechanism for broad — reusing `bynk-lsp`'s
already-shipped `didChangeWatchedFiles` watcher (`bynk-lsp/src/lib.rs:1260`,
`lsp-foundations.md` slice E) as an enumeration source instead of a fresh
`read_dir` per request — is real and shipped, but checked against what it
actually does: `did_change_watched_files`
(`bynk-lsp/src/lib.rs:3378-3455`) reacts to create/change/delete events by
re-scheduling analysis rounds; it does not itself maintain an enumerated
file-list. Turning it into one is a genuine new subsystem — an initial-sweep
vs. first-watch-event race to resolve, a maintained index to invalidate
correctly, and a fallback story for a client that doesn't support dynamic
watcher registration — independently designed and ADR-worthy on its own
terms, not a side effect this track's slice 6 should absorb. Confirmed via
`lsp-foundations.md`'s own retirement summary (`design/archive/retired-tracks.md`):
slice E shipped with **no ADR**, so broad would be this watcher's first
ADR-level treatment, done as a rider on an unrelated track rather than on its
own merits.

**Consequence for §4.** Slice 6 is `read_source`'s content-fallback branch
only, per the original decomposition's narrow-reading bullet — the
conditional "under the broad/under the narrow reading" framing in §4 and §8
collapses to the narrow branch throughout.

### 3.3 The test-harness replacement convention — SETTLED, SHIPPED (slice 3, #1096)

`diagnose_project(&root, &HashMap::new())` (or a small partial overlay) and
`CompileOptions::single(root)`/`::split(root, paths)` are today's "just walk
this directory, I trust the fallback for the rest" test idiom. **Re-counted
under this pass** with a paren-balancing scan (not a single-line grep, which
is what produced the previous, wrong counts below) — figures are still
hand-derived and approximate, not xtask-verified, which is itself evidence
for §3.4's decision to make the real count a CI-checked probe rather than a
number asserted in prose:

- `diagnose_project`/`diagnose_project_with` called with an empty/absent
  overlay (`&HashMap::new()`, a local `HashMap`-aliased `&Map::new()` in two
  `#[cfg(test)]` modules, or the turbofish `&HashMap::<PathBuf, String>::new()`
  spelling): **~72 call sites across 28 files**, all genuinely test-only.
  `diagnose_project_with` itself is **not** test-only, though — it is called
  3 times from production (`bynk-lsp/src/lib.rs:627,1103,3179`, each with a
  real overlay) plus once more internally by `diagnose_project`'s own
  single-tree wrapper (`bynk-ide/src/lib.rs:257`) — only 4 of its 8 real call
  sites are the test empty-overlay idiom this migration targets. The
  originally-recorded "75 bare + 10 `diagnose_project_with` = 85, all
  test-only" figure this doc previously carried was wrong on both counts: the
  bare figure undercounted (missed the `Map`-alias and turbofish spellings),
  and "all test-only" is false for `diagnose_project_with` as a function,
  true only for the specific empty-overlay call shape.
- `CompileOptions::single`/`::split` with no `.sources(...)` anywhere in the
  chained call: **53 call sites across 40 files**. 57 total call sites exist
  project-wide; 4 already chain `.sources(...)` and are out of scope —
  `bynk-driver/src/lib.rs`'s 3 production constructions (`:49,64,103`,
  confirmed chaining `.sources(...)` post-`343b2482`), and **one** site inside
  `bynk-emit/src/project.rs`'s own `#[cfg(test)]` module (line 5858). The
  previously-recorded claim that "`bynk-emit/src/project.rs`'s own three
  production-scope constructions are excluded" was wrong: `project.rs` has no
  three production sites (that count is `bynk-driver`'s, not `project.rs`'s);
  `project.rs` has exactly two test-only sites of its own, and only one of
  them (5858) already chains `.sources(...)`. The other, line 5742
  (`CompileOptions::split(root.to_path_buf(), read_project_paths(&root))`,
  no `.sources(...)`), was wrongly swept into "excluded, already fixed" and is
  actually in scope, in the 53 above and the 40-file count (the 39 files in
  `bynkc/tests`/`bynk/tests` this doc previously named, plus `project.rs`
  itself).

Deleting `discovery.rs`'s fallback means every one of those **~125** sites
(72 + 53 — not the previously-claimed 142) must supply a *complete* sources
map instead of relying on it silently filling gaps.

**Decision.** A new dev-only workspace crate, `bynk-testkit`, not an
extension of `bynk-emit/src/testkit.rs`'s existing `#[cfg(test)]
pub(crate) mod testkit` (`bynk-emit/src/lib.rs:18`). That module is
crate-private by design — its two helpers (`emit_source`, `emit_bundle`) exist
for `bynk-emit`'s own tests only — and cannot serve `bynk-ide`'s inline tests,
`bynk-lsp/tests`, or `bynkc/tests` without either making it `pub` (leaking
`bynk-emit`-internal test helpers as a public surface) or, for `bynk-lsp/tests`
specifically, reaching into `bynk-emit` at all — the same dependency
`bynk-lsp` deliberately doesn't take in production (§3.1). A dev-only crate
sidesteps both: it's a `[dev-dependencies]` entry, invisible to
`fs_below_driver`'s production-only probe, and free to depend on whatever it
needs regardless of what the crate under test depends on in production.

**Built on production discovery, not a reimplementation — by construction.**
`bynk-ide` already exposes `pub fn discover_files(roots: &AnalysisRoots) ->
Vec<PathBuf>` (`bynk-ide/src/lib.rs:234`, calling
`bynk_emit::project::discover_project_files(&roots.lower())`) — the same
resolution production analysis uses. `bynk-testkit`'s core helper is thin
sugar directly over it:

```rust
pub fn read_project_sources(roots: &bynk_ide::AnalysisRoots) -> HashMap<PathBuf, String> {
    bynk_ide::discover_files(roots)
        .into_iter()
        .filter_map(|p| Some((p.clone(), std::fs::read_to_string(&p).ok()?)))
        .collect()
}
```

This closes the doc's own named drift risk (a test silently missing files
because the helper's walk resolves include/exclude differently from
`discover_bynk_files`) **structurally**, not by proof alone: there is no
second walk implementation to drift from the first, because there is only
one. `bynk-testkit` became a dev-dependency of `bynk-lsp` and `bynkc` (which
already dev-depends on `bynk-ide` for its integration tests,
`bynkc/Cargo.toml:60-64` — so the crate-boundary shape has direct precedent)
— **not** `bynk-ide` itself: making `bynk-ide`'s own inline tests dev-depend
on a crate that depends on `bynk-ide` is a cyclic dev-dependency, and Cargo
instantiates two separate copies of the `bynk_ide` crate for it (a real
`E0308` — `crate::AnalysisRoots` and `bynk_ide::AnalysisRoots` become
distinct types — found on the first attempt, not foreseen when this section
was settled). `bynk-ide`'s own inline tests don't need a separate crate at
all — they already have `crate::`-level access to `discover_files`, so their
slice-4 migration writes a tiny private in-crate helper instead.

Proven against one representative call site from three of the four groups
named in §4 (`bynk-lsp/tests`, `bynkc/tests`'s `CompileOptions::single` and
`::split` — the fourth, `bynk-ide`'s own inline tests, is out of scope for
this crate per the paragraph above) before the full migration. That proof
pass — including finding and fixing a real bug: an early version
canonicalised the sources map's keys, which broke
`bynk.project.inconsistent_commons_name`'s path-shape check the moment it
ran against a real multi-root example, because `CompileOptions.sources`
skips filesystem discovery entirely once populated, so the keys' shape
*is* what downstream identity checks see. Fixed to match
`bynk-driver/src/discovery.rs`'s own `sources_for_roots`/`read_bynk_tree`
convention (the proven production populator, #1077/#1081): the literal
discovered path, never canonicalised — is slice 3 (#1096); the full ~120-site
migration is slice 4, and the §3.4 CI guard (originally planned as part of
this slice) moves there too, since slice 4 is what actually needs it.

### 3.4 Migration order and mixed-state safety — SETTLED, incremental

**Decision.** Incremental, sub-sliced by crate in the order the ~125 sites
naturally group: `bynk-ide`'s inline `#[cfg(test)]` modules, then
`bynk-lsp/tests`, then `bynkc/tests`/`bynk/tests`/`bynk-emit`'s own
`#[cfg(test)]` module — each its own PR (§4's slice 5 becomes several,
resolving its own open sub-question). This matches the repo's existing
review culture (a 125-site all-at-once diff is far outside the norm
evidenced by every other slice in this doc's own §4) and gives earlier
signal per straggler.

**Final correction, settled at slice 4's actual completion (not designed in
advance):** no separate `xtask` CI-guard probe was built. The originally
planned mechanism — a `fs_below_driver`-shaped probe counting remaining bare
call sites, checked in as an expected value — would have meant adding a 14th
probe to `xtask/src/greenfield_status.rs`, a module whose own doc scopes it
to "measuring the tree against `design/bynk-greenfield-compiler.md`" — a
different design doc, for a different, already-closed set of R2.x rules.
Bolting a content-ownership-specific, one-time migration-completion check
onto that module would have been exactly the kind of speculative,
purpose-mismatched infrastructure this repo's own review culture flags
elsewhere ([ADR 0319](../decisions/0319-positive-nonnegative-inrange-fold-declined.md)).

More importantly, it turned out to be unnecessary: once slice 5 deletes
`discovery.rs`'s content-reading fallback, a straggler bare call site stops
being a *silent* problem on its own — with no fallback left to fill an
empty/incomplete sources map, `read_source` has only the overlay to answer
from, so a missed conversion fails its own test loudly (empty or missing
file content breaks the test's assertions) the moment slice 5 ships. Slice
5's own deletion **is** the loud-failure mechanism the original design
question was solving for; a separate static-analysis probe would have been
redundant defence-in-depth for a migration that, by the time slice 4
actually finished (all identified sites converted across `bynk-ide`,
`bynk-lsp/tests`, `bynkc/tests`/`bynk/tests`, and `bynk-emit`'s own
`#[cfg(test)]` module), had nothing left for a probe to guard against.
`discovery.rs`'s fallback (slice 5) is deleted now that migration is
complete.

### 3.5 `AnalysisRoots::lower()`'s own `bynk.toml` read — SETTLED, SHIPPED (slice 2, #1094)

**Decision.** Independent of §3.1's correction (below) — this fix stands on
its own merits, not by "falling out of" a `ProjectDirs`/`resolve_dirs` type
that turned out not to be built. `lower()` (`bynk-ide/src/lib.rs:211`)
becomes `fn lower(&self, overlay: &HashMap<PathBuf, String>) ->
bynk_emit::project::Roots`, threading `overlay` into the already-overlay-aware
`bynk_emit::project::try_read_project_paths_with` (`bynk-emit/src/project/paths.rs:162`,
already `pub`) instead of the disk-only `read_project_paths`, the same fix
`343b2482` already applied on the CLI side.

**Correction found under implementation:** this section's original text
claimed "`lower()`'s only caller inside this crate is `diagnose_project_with`"
— wrong, `discover_files` (`bynk-ide/src/lib.rs:234`) calls it too, with no
overlay of its own to give. Slice 2 ships `discover_files` passing an empty
overlay at its call site (`&HashMap::new()`) — its own `bynk.toml` freshness
is unchanged (still disk-only, per §3.2's enumeration-stays-out-of-scope
reading); only `diagnose_project_with` (`bynk-ide/src/lib.rs:266`, which
already receives a real `overlay` from every caller, including `bynk-lsp`'s
`run_project_diagnostics`) actually changes behaviour. Kept as its own slice
because it changes the *already-shipped*,
production-facing analysis path `diagnose_project_with` — a smaller, more
isolated change than the seam slice 0 shipped, and safer to land separately.

## 4. Candidate slice decomposition

Renumbered after §3.1's implementation-time correction merged the original
slices 0 and 1 (a scaffolding-only "land the map, no caller yet" slice was
not independently shippable — see the superseding
[ADR 0325](../decisions/0325-content-ownership-seam-simplification.md)).
Slices 2–6 keep their original substance, renumbered down by one.

- **Slice 0 — `bynk-lsp`'s sweep + `for_each_unit` takes content. Shipped
  (#1089, #1090).** `bynk-lsp/src/content.rs`'s `sweep_project_content`,
  built directly on the already-public `bynk_ide::discover_files` (no new
  `bynk-ide` type — §3.1); `Backend::project_content` (overlay-then-sweep,
  mirroring `project_files`'s then-existing current-file exclusion).
  `bynk-ide/src/completion.rs`'s `for_each_unit`/`cached_project_unit` become
  content-supplying (cache keyed on content equality, not disk mtime/len);
  every `files: Option<&[PathBuf]>` signature in
  `completion.rs`/`signature_help.rs` (11 call sites in `completion.rs`, 2 in
  `signature_help.rs`) becomes `Option<&HashMap<PathBuf, String>>`.
  `bynk-lsp/Cargo.toml` gains the dependency-exclusion comment
  `bynk-ide/src/lib.rs:31` already claimed existed.
- **Slice 1 — `symbols.rs` cross-file lookups take content. Shipped
  (#1092).** `find_declaration_cross_file`/`describe_symbol_cross_file`
  (`bynk-ide/src/symbols.rs`) take pre-read `(path, content)` maps instead of
  re-reading inside their loops (a `sorted_paths` helper preserves the
  original deterministic "first hit in path order" semantics `HashMap`
  iteration alone wouldn't give). `Backend::project_files` (bare paths, its
  last remaining caller) is deleted entirely — `Backend::project_content`
  (slice 0) is now the sole project-enumeration entry point.
  `fs_below_driver`'s `bynk-ide` count reaches 0.
- **Slice 2 — `AnalysisRoots::lower()`'s `bynk.toml` read joins the overlay.
  Shipped (#1094).** (§3.5) — mirrors `343b2482`'s CLI-side fix on the LSP
  side; `discover_files`'s own call to `lower()` passes an empty overlay,
  unchanged behaviour (§3.5's correction).
- **Slice 3 — `bynk-testkit`, proved narrow. Shipped (#1096).** The new
  dev-only crate (`bynk-lsp`/`bynkc` dev-dependency, not `bynk-ide` — §3.3's
  correction) and its `read_project_sources`/`compile_options_single`/
  `compile_options_split` helpers, proved against one representative call
  site each in `bynk-lsp/tests`, `bynkc/tests`'s `CompileOptions::single` use,
  and `::split` use. The §3.4 CI guard (the call-site-count probe) moves to
  slice 4 — this slice's job was proving the crate, not enforcing the
  migration yet.
- **Slice 4 — migrate the remaining ~120 test call sites, sub-sliced by
  crate** (§3.4, spine sub-issue #1098, itself parenting one sub-issue per
  crate): **sub-slice 1 shipped** — `bynk-ide`'s own 18 inline-test sites
  (`architecture.rs`, `locals_nav.rs`, `sequence.rs`, `wire_contract.rs`),
  via an inline `#[cfg(test)] mod testkit { … }` block at the end of
  `lib.rs` — not a separate file, and not `bynk-testkit` (§3.3's cyclic-
  dependency correction). A first attempt used a separate
  `bynk-ide/src/testkit.rs` file and moved `fs_below_driver`'s `bynk-ide`
  count from 0 back to 1: the probe only recognises an inline `#[cfg(test)]
  mod name { … }` block as test-scope, not a whole file gated by its
  *declaration* in a different file. Fixed by inlining. **Sub-slice 2
  shipped** — all 25 `bynk-lsp/tests` sites (21 bare `diagnose_project`
  across `architecture_request.rs`, `code_actions.rs` ×9, `document_link_fixture.rs`,
  `hover_references.rs` ×2, `interpolation_lsp.rs` ×4, `project_model.rs`,
  `renderer_kind_coverage.rs`, `wire_contract_request.rs` ×2; 4
  `diagnose_project_with` across `project_model.rs` ×3 —
  including its shared `rel_files` helper, whose single conversion fixed 8
  further tests indirectly, higher leverage than the per-site count alone
  suggests — and `sequence_request.rs` ×1) via `bynk-testkit`. **Sub-slice 3
  shipped** — the remaining 78 sites: 27 `diagnose_project` sites across 13
  `bynkc/tests` files, 50 `CompileOptions::single`/`::split` sites across 38
  files in `bynkc/tests`/`bynk/tests` (via `bynk-testkit`, now also a
  dev-dependency of `bynk`), and `bynk-emit/src/project.rs`'s own one bare
  `CompileOptions::split` site, via a private in-crate helper mirroring
  `bynk-ide`'s (§3.3's cyclic-dependency correction applies here too —
  `bynk-emit`'s own `#[cfg(test)]` module can't depend on `bynk-testkit`
  either) added inside its existing inline `mod tests { … }` block. No
  separate §3.4 CI guard was built in the end — see §3.4's final correction.
- **Slice 5 — delete `discovery.rs`'s content-reading fallback branch**
  (§3.2, narrow: the enumeration walk and `project.rs`'s `use std::fs;` stay,
  serving `discover_bynk_files`). `fs_below_driver` reaches 0 for `bynk-ide`
  (already true after slice 1) and reads 2 for `bynk-emit` — both
  `discovery.rs`'s enumeration walk and `project.rs`'s import serving it —
  until the probe amendment named in §3.2 lands as its own follow-on.

## 5. Front-loaded ADR candidates — landed via `design/pending/settle-content-ownership-track.md`

- **§3.1 — the `bynk-ide`-exposed seam type.** [ADR 0322](../decisions/0322-content-ownership-seam-type.md)
  settled `ProjectDirs`/`resolve_dirs`; superseded under implementation —
  no new type was needed (see `design/pending/content-ownership-seam-simplification.md`,
  pre-stamp). Recorded here rather than edited away: the front-load process
  worked as intended (the hard-to-reverse call was made explicit before
  slicing), even though the call itself needed a correction once built.
- **§3.2 — R2.3's ambient-filesystem ban is content-scoped; enumeration stays
  below the driver.** Decides the track's actual production-code scope going
  in, and `fs_below_driver`'s achievable floor for `bynk-emit`.
- **§3.3 — cross-crate test fixtures get a new `bynk-testkit` crate, built
  on production discovery.** The ~120 call sites will depend on it once slice 4
  lands; changing the crate boundary afterward is another full migration.

## 6. Threat model

None, because this is an internal architectural and testability property —
one explicit, injectable view of file content, replacing several ad hoc disk
reads — not a security or safety boundary. No untrusted input crosses a new
trust boundary: the content a request resolves against is still exactly the
project's own files, sourced from the client's own open buffers or its own
disk, exactly as today. No capability changes what an agent or handler is
authorised to read; this only changes which Rust function performs the read
and when it's considered fresh.

## 7. Slice status

- [x] Slice 0 — `bynk-lsp`'s sweep + `for_each_unit` takes content (#1089;
      merged from the original slices 0+1 under §3.1's implementation-time
      correction)
- [x] Slice 1 — `symbols.rs` cross-file lookups take content (#1092;
      `Backend::project_files` retired)
- [x] Slice 2 — `AnalysisRoots::lower()`'s `bynk.toml` read joins the overlay
      (#1094)
- [x] Slice 3 — `bynk-testkit`, proved narrow (#1096)
- [x] Slice 4 — migrate the remaining ~120 test call sites, sub-sliced by
      crate (#1098) — all 3 sub-slices shipped (`bynk-ide`'s own inline
      tests, 18 sites; `bynk-lsp/tests`, 25 sites; `bynkc/tests`/`bynk/tests`/
      `bynk-emit`, 78 sites; 121 total)
- [x] Slice 5 — delete `discovery.rs`'s content-reading fallback branch
      (#1102) — plus three corrections found only under implementation:
      `run_project_diagnostics`/`type_receiver`'s open-buffers-only overlay
      (production, not test — the loudest of the three), `discover_files`/
      `AnalysisRoots::lower`'s `bynk.toml` read moving to callers above the
      driver (R2.3), and a deliberate, documented carve-out for adapter
      `.binding.ts` reads (never this track's target; see §1, §3.2).

## 8. Done when

`cargo xtask greenfield-status`'s `fs_below_driver` reads 0 for `bynk-ide`
(`bynk-fmt` is already 0); for `bynk-emit` it reads 3 — `discovery.rs`'s
enumeration walk and its (newly-carved-out, deliberate) adapter-binding
read, plus `paths.rs`'s own `bynk.toml` disk read (the same "R2.3 forbids
fixing this below the driver, so the read moves to/above the process edge"
reasoning as `discover_files`'s, for `read_project_paths`'s non-overlay
callers) — until the §3.2 probe-precision follow-on lands and the floor
becomes named-and-intentional rather than a residual count.
`bynk-emit/src/project/discovery.rs` has no content-reading disk fallback
left for `.bynk` project sources. A behaviour-driven test — mirroring ADR
0202's "drive a real `Backend` through `didChange` → request" style, not a
static shape assertion — demonstrates that an unsaved edit in file A is
visible to a completion triggered from file B
(`an_unsaved_edit_in_one_file_is_visible_to_completion_in_another`,
`bynk-lsp/src/lib.rs`). #1077 and #1079 closed directly on `main` before
this track's spine even opened (see #1077's own closing comment); their
remaining scope became this track's actual charter, re-verified against
current `main` rather than their original text (§1). This doc retires next
— the retirement PR closes spine issue #1086.
