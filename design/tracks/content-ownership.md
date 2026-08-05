# Project content ownership — `bynk-lsp` becomes the sole reader of `.bynk` source content

- **Status:** Settling (draft). This is a settling-draft doc per
  [ADR 0167](../decisions/0167-feature-tracks-run-github-native.md): the
  **spine issue** is
  [#1086](https://github.com/accuser/bynk/issues/1086); this doc lands via a
  draft PR referencing it (*"Part of #1086, #1079"*, never `Closes`). No
  slices have shipped yet — every open question below is genuinely open.
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
- **Surface not yet settled.** Five genuinely open questions (§3): the shape
  of the new `bynk-ide`-exposed seam (§3.1); whether R2.3's "no ambient
  filesystem" also bans directory *enumeration*, not just content reads
  (§3.2); what replaces a ~145-site test convention without turning every
  test file into boilerplate (§3.3); migration order and mixed-state safety
  (§3.4); and `AnalysisRoots::lower()`'s own `bynk.toml` read (§3.5). §3.1–§3.3
  are the three §5 front-loads as ADR candidates.
- **Security/safety boundary — no.** This is an architectural correctness and
  testability property, not an attacker-facing boundary; §6 explains why.

## 3. Open design questions

### 3.1 The `bynk-ide`-exposed seam shape

`bynk-lsp` needs resolved include/exclude project directories to do its own
disk sweep. It cannot get them from `bynk-emit::project::Roots` directly
(deliberate: `bynk-lsp` does not depend on `bynk-emit` — no dependency-comment
currently states this in `bynk-lsp/Cargo.toml`, so landing that rationale in
writing is itself part of this track, not an existing constraint to merely
cite). Re-exporting `Roots` through `bynk-ide` would relocate the violation
rather than close it, since `bynk-ide` is itself one of the three crates
`fs_below_driver` probes.

`AnalysisRoots::lower()` (`bynk-ide/src/lib.rs:211`) is the current
`Roots`-producing function — it takes `bynk-ide`'s own `AnalysisRoots` enum
(single file vs. project) and turns it into `bynk-emit`'s `Roots`. The
candidate shape: a new, narrower type — resolved absolute include/exclude
directory lists, no `Roots` fields beyond that — that `bynk-ide` exposes
*instead of* `Roots`, and that both `bynk-lsp`'s new sweep and
`AnalysisRoots::lower()` itself can build from. Settling this doc means
naming that type's actual fields, not just gesturing at "narrower than
`Roots`".

### 3.2 Does enumeration count as "ambient filesystem" under R2.3?

`discover_bynk_files` (`bynk-emit/src/project/discovery.rs:286`) walks the
directory tree via `fs::read_dir` to find *which* `.bynk` files exist — it
never reads their content. R2.3's own wording — "the compiler's only view of
file *contents*" — is about content specifically. Two readings:

- **Narrow (content only).** `discover_bynk_files`'s enumeration walk can stay
  below the driver indefinitely; only `read_source`'s content-reading fallback
  branch is in this track's scope. `fs_below_driver`'s probe would need
  amending to stop flagging pure-enumeration functions, or `discovery.rs`
  would still show non-zero after this track ships and the probe's own
  precision becomes a follow-on.
- **Broad (enumeration too).** `discover_bynk_files` also has to move above
  the driver — a bigger structural change, since "which files exist" changes
  on every save/create/delete, not just on open-buffer edits, so `bynk-lsp`'s
  sweep needs its own re-enumeration story (a filesystem watcher already
  exists for `didChangeWatchedFiles` per `lsp-foundations.md`'s slice E — this
  reading would reuse it as the enumeration source instead of a fresh
  `read_dir` per request).

This needs a real decision, not a default — it changes whether §4's slice 6 is
one deletion or two, and it's the first front-loaded ADR (§5).

### 3.3 The test-harness replacement convention

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

Candidate: a `testkit`-module helper (mirroring the existing `testkit.rs`
pattern R2.3's own table already credits as landed for `CompileOptions.sources`)
that performs the walk-and-read itself — real `fs::read_dir` +
`fs::read_to_string`, but *at test-fixture-setup time, in test code*, not in
`bynk-emit` — and returns a populated `HashMap`/`CompileOptions` a test can
pass through the normal overlay-only path. Behaviourally identical to today's
call sites (same directory in, same complete view out); the difference is
*where* the disk read lives; test code is not `fs_below_driver`-gated.

This has to be genuinely a drop-in replacement — a one-line helper-name swap,
not a per-test rewrite — or 142 call sites will not migrate cleanly and this
track stalls at slice 5. Proving that on a representative handful of call
sites (§4, slice 4) before the full migration (slice 5) is why those are
separate slices, not one.

The drop-in property holds only while the helper's walk resolves
include/exclude exactly as `discover_bynk_files` does. If slice 4
reimplements the walk instead of reusing that resolution, the two can drift —
and the failure mode is the hardest kind to notice: a test that passes while
silently missing files, not a loud error. Slice 4's proof pass (§4) needs to
cover this, not just "does the helper compile and one test pass".

### 3.4 Migration order and mixed-state safety

Two shapes for slices 4–6: an **incremental** deletion (delete
`discovery.rs`'s fallback per-call-site as each caller migrates, with a
temporary hard error — not a silent empty read — on any path the migration
hasn't reached yet, so a straggler fails loudly in CI rather than passing by
accident on a still-fallback-covered case), or an **all-at-once flip** once
the replacement helper (§3.3) exists and every call site has been mechanically
converted in one pass. The incremental shape gives earlier signal and smaller
review diffs; the all-at-once shape avoids a window where two conventions
coexist and a new test can accidentally pick the wrong one. Settling this
decides whether §4's test-harness half is one slice or three.

### 3.5 `AnalysisRoots::lower()`'s own `bynk.toml` read

`read_project_paths` (`bynk-ide/src/lib.rs:216`, inside the `AnalysisRoots::Project`
branch of `lower()`) reads `bynk.toml` disk-only — the same
always-empty-overlay gap `343b2482` closed on the CLI side (`project_options`/
`try_project_options` in `bynk-driver`, closing the CLI-path half of #1077).
#1084's investigation found this isn't independently fixable the way the CLI
one was: `diagnose_project_with`'s only caller-supplied input today is a
*partial* buffer overlay, so there is nothing complete yet to hand
`try_read_project_paths_with`. The working hypothesis is that this falls out
naturally once §3.1's content-supplying seam exists (§4, slice 3) — confirming
that, rather than assuming it, is part of settling this doc.

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
- **Slice 4 — the test-harness replacement helper, proved narrow** (§3.3): the
  `testkit` walk-and-read helper, landed and proved against a small
  representative sample of call sites (one from each of `bynk-ide`'s inline
  tests, `bynk-lsp/tests`, `bynkc/tests`'s `CompileOptions::single` use, and
  `::split` use) before committing to the full migration.
- **Slice 5 — migrate the remaining ~140 test call sites** (142 minus slice
  4's proof-of-concept handful) to the slice-4 helper. §3.4 decides whether
  this is one slice or several.
- **Slice 6 — delete `discovery.rs`'s content fallback.** Under §3.2's
  **broad** reading, the enumeration walk goes too, `project.rs`'s `use
  std::fs;` becomes genuinely dead and is removed, and `fs_below_driver`
  reaches 0 for `bynk-emit` — combined with slice 2, the probe reads 0 end to
  end (only `bynk-fmt`'s already-0 count remains in the table). Under the
  **narrow** reading, `discovery.rs` keeps its `fs::read_dir` enumeration
  walk (and `project.rs`'s import stays live to serve it), so `bynk-emit`
  stays at 2 — this slice only removes the *content*-reading fallback branch,
  and the probe's own precision (§3.2) becomes a named follow-on, not a
  defect of this slice.

Provisional — genuinely settling this doc (closing §3.1–§3.5 under review) may
reorder or merge these before slice 0 is cut as its own increment proposal.

## 5. Front-loaded ADR candidates

- **§3.1 — the `bynk-ide`-exposed seam type.** Hard to reverse once
  `bynk-lsp`'s sweep and every migrated test call site depend on its shape.
- **§3.2 — whether R2.3 covers enumeration as well as content.** Decides the
  track's actual production-code scope going in; discovering this slice by
  slice instead would mean re-scoping mid-track.
- **§3.3 — the test-harness replacement convention.** 142 call sites will
  depend on it once slice 5 lands; changing the convention afterward is
  another full migration, not a fix-up.

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

`cargo xtask greenfield-status`'s `fs_below_driver` reads 0 for `bynk-ide`, and
for `bynk-emit` reads 0 under §3.2's broad reading or reads only
`discovery.rs`'s enumeration walk (with the probe-precision follow-on named,
not silently left) under the narrow reading (`bynk-fmt` is already 0);
`bynk-emit/src/project/discovery.rs` has no content-reading disk fallback left
in either case, and no enumeration walk either if §3.2 resolves broad; and a
behaviour-driven test — mirroring ADR 0202's
"drive a real `Backend` through `didChange` → request" style, not a static
shape assertion — demonstrates that an unsaved edit in file A is visible to a
completion/hover/go-to-declaration triggered from file B. #1077 and #1079
close as part of the slice that lands each half; this doc retires once §4's
slices have all shipped.
