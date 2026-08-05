# Project content ownership — `bynk-lsp` becomes the sole reader of `.bynk` source content

- **Status:** Slicing readiness — settled, no slices shipped yet. This doc's
  first pass merged still carrying every §3 question open (PR #1087, merged
  as ready-for-review without the review actually testing that assertion —
  exactly the failure mode `design/tracks/README.md`'s lifecycle step 2
  warns about, so the doc's real phase stayed **Settling** past that merge).
  This re-settling pass closes §3.1–§3.5 for real: §3.1 and §3.5 close
  together (the seam type's `overlay` parameter is what makes §3.5's `bynk.toml`
  read fall out naturally, confirming rather than just hypothesising the
  original doc's own guess); §3.2 settles narrow; §3.3 settles the crate
  boundary and defers the exact helper shape to slice 4, as originally
  planned; §3.4 settles incremental with a structural CI guard. The three
  front-loaded ADRs (§5) land in this same pass via
  `design/pending/settle-content-ownership-track.md` (pre-stamp). Spine issue
  [#1086](https://github.com/accuser/bynk/issues/1086) stays open; slice 0 is
  cut as its own increment-proposal sub-issue once this pass merges.
- **Realises:** R2.3 (`../bynk-greenfield-compiler.md`, its rules table at
  line 2515) — *"no ambient filesystem or global state; `Sources` is
  constructed once, at the process edge, and is the compiler's only view of
  file contents"* — specifically the `bynk-ide` row of `fs_below_driver`, the
  probe that checks it (`../greenfield-status.md:12`), open since T0.7
  (#1006/#1012).
- **Posture:** Feature track per
  [ADR 0076](../decisions/0076-feature-track-posture.md). Qualifies on two
  axes (§2): it is multi-increment (production-code seam, then a ~145-site
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
  `symbols.rs` alone, because ~145 test call sites across `bynk-ide`,
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

- **Multi-increment.** The production-code half (§4, slices 0–3: a new seam
  type, `bynk-lsp`'s sweep, migrating ~13 call sites) and the test-harness half
  (§4, slices 4–6: replacing the `diagnose_project(&root, &HashMap::new())`
  convention across ~145 call sites, then deleting the fallback) are each too
  large for one delete-on-merge proposal — and the second cannot start until
  the first has shipped something for tests to call instead.
- **Surface not settled at the doc's first merge — is now.** Five genuinely
  open questions (§3), all closed by this re-settling pass: the shape of the
  new `bynk-ide`-exposed seam (§3.1, settled: `ProjectDirs`/`resolve_dirs`);
  whether R2.3's "no ambient filesystem" also bans directory *enumeration*,
  not just content reads (§3.2, settled narrow); what replaces a ~142-site
  test convention without turning every test file into boilerplate (§3.3,
  settled: a new `bynk-testkit` crate, exact helper shape deferred to slice
  4); migration order and mixed-state safety (§3.4, settled incremental with
  a structural CI guard); and `AnalysisRoots::lower()`'s own `bynk.toml` read
  (§3.5, settled — falls out of §3.1). §3.1–§3.3 are the three §5 front-loads,
  landed as ADRs via `design/pending/settle-content-ownership-track.md`.
- **Security/safety boundary — no.** This is an architectural correctness and
  testability property, not an attacker-facing boundary; §6 explains why.

## 3. Open design questions

### 3.1 The `bynk-ide`-exposed seam shape — SETTLED

**Decision.** `bynk-ide` gains a new public type and method, alongside the
existing `AnalysisRoots`/`lower()`/`discover_files()` (`bynk-ide/src/lib.rs:192-236`):

```rust
/// Slice 0: the resolved include/exclude directories a project analysis
/// walks, narrow enough for `bynk-lsp`'s own disk sweep without re-exporting
/// `bynk_emit::project::Roots` — `bynk-ide` is itself one of the three
/// `fs_below_driver`-probed crates, so re-exporting `Roots` would relocate
/// the violation rather than close it.
#[derive(Debug, Clone)]
pub struct ProjectDirs {
    pub project_root: PathBuf,
    pub include: Vec<PathBuf>,
    pub exclude: Vec<PathBuf>,
}

impl AnalysisRoots {
    pub fn resolve_dirs(&self, overlay: &HashMap<PathBuf, String>) -> ProjectDirs { .. }
}
```

`resolve_dirs` mirrors `lower()`'s match but threads `overlay` into
`bynk_emit::project::try_read_project_paths_with(root, overlay)`
(`bynk-emit/src/project/paths.rs:156`, already overlay-aware, already `pub`)
instead of the disk-only `read_project_paths(root)` `lower()` calls today —
this is what closes §3.5 in the same motion (below). For `SingleTree`,
`include = [root]`, `exclude = []`; for `Project`, `include`/`exclude` come
from the resolved `ProjectPaths`. `ProjectDirs` structurally mirrors
`bynk_emit::project::ProjectPaths` but is a distinct, `bynk-ide`-owned type —
a re-export would fail the same way re-exporting `Roots` would (`bynk-ide`'s
own `fs_below_driver` exposure).

`bynk-lsp`'s new sweep (slice 0) calls `AnalysisRoots::Project(root)
.resolve_dirs(&overlay)`, walks `include` (honouring `exclude`) for
`.bynk` files not already in the overlay, and reads them — giving `bynk-lsp`
a complete `(path, content)` map without depending on `bynk-emit` at all,
since `bynk-ide` (which `bynk-lsp` already depends on, per its
`Cargo.toml`'s dependency comment block) is the crate doing the
`bynk-emit`-facing work.

**Loose end this decision surfaces, not introduces.** `bynk-ide/src/lib.rs:31`'s
doc comment on `pub use bynk_emit::project::ContextSequenceInfo` already
asserts "see `bynk-lsp/Cargo.toml`'s dependency comment" for the
does-not-depend-on-`bynk-emit` rationale — no such comment exists there today
(checked: `bynk-lsp/Cargo.toml`'s dependency block explains what each
dependency is *for*, not what's deliberately excluded). Slice 0 adds that
comment as part of landing `ProjectDirs`/`resolve_dirs`, making the existing
cross-reference true instead of dangling.

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

### 3.3 The test-harness replacement convention — SETTLED (crate boundary), shape deferred to slice 4

`diagnose_project(&root, &HashMap::new())` (or a small partial overlay) and
`CompileOptions::single(root)`/`::split(root, paths)` are today's "just walk
this directory, I trust the fallback for the rest" test idiom — measured
current counts (§7 of the investigation this doc is built from, re-verified
under review): 75 bare `diagnose_project` calls plus 10 `diagnose_project_with`
calls (85 total, **all test-only**, none in production — confirmed against
each hit's enclosing `#[cfg(test)]` boundary) across `bynk-ide`'s inline
`#[cfg(test)]` modules, `bynk-lsp/tests`, and `bynkc/tests`; separately, 57
`CompileOptions::single`/`::split` call sites across 39 files in
`bynkc/tests`/`bynk/tests` (`bynk-driver/src/lib.rs` and
`bynk-emit/src/project.rs`'s own three production-scope constructions are
excluded — they already chain `.sources(...)` post-`343b2482` and are not
part of this migration). Deleting `discovery.rs`'s fallback means every one of
those 142 sites must supply a *complete* sources map instead of relying on it
silently filling gaps.

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
one. `bynk-testkit` becomes a dev-dependency of `bynk-ide`, `bynk-lsp`, and
`bynkc` (which already dev-depends on `bynk-ide` for its integration tests,
`bynkc/Cargo.toml:60-64` — so the crate-boundary shape has direct precedent).

**Deferred to slice 4, as originally planned.** The exact call-site-facing
shape — a `read_project_sources`-style helper for the 85
`diagnose_project(&root, &HashMap::new())`/`diagnose_project_with` sites vs.
a `CompileOptions`-returning helper (needs `bynk-testkit` to also dev-depend
on `bynk-emit`, itself unproblematic for a dev-only crate) for the 57
`CompileOptions::single`/`::split` sites — is proven against one
representative call site from each of the four groups named in §4 before the
full migration. That proof pass is what slice 4 is *for*; this decision only
settles that both helpers live in one new crate built the stated way, not
their final signatures.

### 3.4 Migration order and mixed-state safety — SETTLED, incremental

**Decision.** Incremental, sub-sliced by crate in the order the 142 sites
naturally group: `bynk-ide`'s inline `#[cfg(test)]` modules, then
`bynk-lsp/tests`, then `bynkc/tests`/`bynk/tests` — each its own PR (§4's
slice 5 becomes several, resolving its own open sub-question). This matches
the repo's existing review culture (a 142-site all-at-once diff is far
outside the norm evidenced by every other slice in this doc's own §4) and
gives earlier signal per straggler.

Rather than a runtime hard-error in `discovery.rs` (impossible to scope
correctly — the fallback can't tell a "not yet migrated" caller from a
legitimate one without call-site-level plumbing that doesn't exist), the
loud-failure mechanism is a **structural CI guard**: an `xtask` probe,
added alongside slice 4, that counts remaining
`diagnose_project(&root, &HashMap::new())`/`diagnose_project_with(_, &HashMap::new())`/
bare `CompileOptions::single`/`::split` (no `.sources(...)` chained) call
sites outside `bynk-testkit` itself, and fails CI if that count doesn't
strictly decrease once slice 5 begins. This is the same idiom
`fs_below_driver` and `decisions_index` already use in this repo — a
structural drift guard, not a new runtime code path — so a straggler is a
CI failure on the stalled PR, not a silent pass. `discovery.rs`'s fallback
(slice 6) is deleted only once the probe reads zero.

### 3.5 `AnalysisRoots::lower()`'s own `bynk.toml` read — SETTLED, confirmed (falls out of §3.1)

**Decision.** Confirmed, not just hypothesised: `AnalysisRoots::resolve_dirs`
(§3.1) already threads `overlay` into `try_read_project_paths_with` instead
of the disk-only `read_project_paths`. Slice 3 (unchanged position in §4)
applies the identical change to `lower()` itself — `fn lower(&self, overlay:
&HashMap<PathBuf, String>) -> bynk_emit::project::Roots`, threading `overlay`
the same way, with `diagnose_project_with` (`bynk-ide/src/lib.rs:266`, which
already receives an `overlay` argument from every caller including
`bynk-lsp`'s `run_project_diagnostics`) passing its own overlay through to
the now-updated `lower()` call. `lower()`'s only caller inside this crate is
`diagnose_project_with`, so the signature change is contained. Kept as its
own slice (3, not folded into slice 0) because it changes the *already-shipped*,
production-facing analysis path `diagnose_project_with` — proving the new
type in isolation (slice 0) before touching that path (slice 3) is the lower-
risk order.

## 4. Candidate slice decomposition

- **Slice 0 — the seam type + `bynk-lsp`'s disk sweep.** Land §3.1's narrow
  directory type and a `bynk-lsp`-local walk that builds one complete
  `(path, content)` map per analysis round: open-buffer overlay first, a real
  disk read for everything else. No caller changes yet — this slice only
  makes the map exist and gets exercised by a direct test of the sweep
  itself.
- **Slice 1 — `for_each_unit` takes content.**
  `for_each_unit(doc_text: &str, files: Option<&[PathBuf]>, ...)`
  (`bynk-ide/src/completion.rs:1557`) becomes content-supplying; its 11
  call sites in `completion.rs` and 2 in `signature_help.rs` get content
  through the slice-0 map instead of a bare path list. `cached_project_unit`'s
  three `std::fs` references go away entirely.
- **Slice 2 — `symbols.rs` cross-file lookups take content.**
  `find_declaration_cross_file`/`describe_symbol_cross_file`
  (`bynk-ide/src/symbols.rs:1481,1507`) take pre-read `(path, content)` pairs
  instead of re-reading inside their loops. `fs_below_driver`'s `bynk-ide`
  count reaches 0.
- **Slice 3 — `AnalysisRoots::lower()`'s `bynk.toml` read joins the overlay**
  (§3.5), now that slice 0's complete map exists to draw from — mirrors
  `343b2482`'s CLI-side fix on the LSP side.
- **Slice 4 — `bynk-testkit`, proved narrow** (§3.3): the new dev-only crate
  and its `read_project_sources` helper, proved against a small
  representative sample of call sites (one from each of `bynk-ide`'s inline
  tests, `bynk-lsp/tests`, `bynkc/tests`'s `CompileOptions::single` use, and
  `::split` use) before committing to the full migration. The §3.4 CI guard
  (the call-site-count probe) is added here too, so slice 5 has something to
  fail against from its first sub-slice.
- **Slice 5 — migrate the remaining ~140 test call sites, sub-sliced by
  crate** (§3.4): `bynk-ide`'s inline tests, then `bynk-lsp/tests`, then
  `bynkc/tests`/`bynk/tests`, each its own PR.
- **Slice 6 — delete `discovery.rs`'s content-reading fallback branch**
  (§3.2, narrow: the enumeration walk and `project.rs`'s `use std::fs;` stay,
  serving `discover_bynk_files`). `fs_below_driver` reaches 0 for `bynk-ide`
  (already true after slice 2) and reads 2 for `bynk-emit` — both
  `discovery.rs`'s enumeration walk and `project.rs`'s import serving it —
  until the probe amendment named in §3.2 lands as its own follow-on.

## 5. Front-loaded ADR candidates — landed via `design/pending/settle-content-ownership-track.md`

- **§3.1 — the `bynk-ide`-exposed seam type (`ProjectDirs`/`resolve_dirs`).**
  Hard to reverse once `bynk-lsp`'s sweep and every migrated test call site
  depend on its shape.
- **§3.2 — R2.3's ambient-filesystem ban is content-scoped; enumeration stays
  below the driver.** Decides the track's actual production-code scope going
  in, and `fs_below_driver`'s achievable floor for `bynk-emit`.
- **§3.3 — cross-crate test fixtures get a new `bynk-testkit` crate, built
  on production discovery.** 142 call sites will depend on it once slice 5
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

- [ ] Slice 0 — the seam type + `bynk-lsp`'s disk sweep
- [ ] Slice 1 — `for_each_unit` takes content
- [ ] Slice 2 — `symbols.rs` cross-file lookups take content
- [ ] Slice 3 — `AnalysisRoots::lower()`'s `bynk.toml` read joins the overlay
- [ ] Slice 4 — the test-harness replacement helper, proved narrow
- [ ] Slice 5 — migrate the remaining ~140 test call sites
- [ ] Slice 6 — delete `discovery.rs`'s content fallback

## 8. Done when

`cargo xtask greenfield-status`'s `fs_below_driver` reads 0 for `bynk-ide`
(`bynk-fmt` is already 0); for `bynk-emit` it reads 2
(`discovery.rs`'s enumeration walk, `project.rs`'s import serving it) until
the §3.2 probe-precision follow-on lands and the floor becomes named-and-
intentional rather than a residual count. `bynk-emit/src/project/discovery.rs`
has no content-reading disk fallback left; a
behaviour-driven test — mirroring ADR 0202's
"drive a real `Backend` through `didChange` → request" style, not a static
shape assertion — demonstrates that an unsaved edit in file A is visible to a
completion/hover/go-to-declaration triggered from file B. #1077 and #1079
close as part of the slice that lands each half; this doc retires once §4's
slices have all shipped.
