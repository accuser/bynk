# LSP foundations — the project model, the freshness contract, and the lifecycle the feature surface rests on

- **Status:** Adopted — direction settled by the merge of the settling PR
  (#641). Adoption is **not** build authorisation: a slice is approved to build
  only when its own proposal is `accepted`. **Slice 0 shipped (v0.175, ADR
  0198)** and **slice A shipped (v0.178, ADR 0201)** — the LSP now analyses
  exactly the project `bynkc` compiles, closing **Q1** and **Q2** (the latter
  re-scoped to slice 0 by #649). **Q5 is settled (model first)**, and the model
  led as it decided. **Slice B shipped (v0.179, ADR 0202)** — the freshness
  contract Q3 settled: an index-backed request refreshes to the current buffer,
  never answers against stale text. **Slice C shipped (v0.180)** — the `[lib]`
  seam (Q6 settled: in-process; no ADR, a refactor). All open questions are
  settled. **Slice D shipped (v0.182, ADR 0204)** — real multi-root: a
  project-root-keyed state map, routing by the file's nearest `bynk.toml` (Q4),
  `did_change_workspace_folders`, and the advertised workspace-folders capability
  made true. **Slice E shipped (v0.183)** — startup analysis (a `bynk.toml`
  tree-walk warms every project on activation) and server-side dynamic
  `didChangeWatchedFiles` registration, so any client is notified. Remaining is
  **F** (one scheduler) and **G** (doc consolidation, no bump) — neither gated.
  Live state on the track's **spine issue**,
  [#640](https://github.com/accuser/bynk/issues/640)
  ([ADR 0167](../decisions/0167-feature-tracks-run-github-native.md)).
- **Realises:** `design/bynk-tooling-roadmap.md` §1–§2 (the LSP's current state
  and roadmap — whose A-0 "foundation" is the *semantic index*, shipped v0.25;
  this track is the foundation under *that*) and the feature spec that outlived
  the retired `lsp.md` track,
  [`../bynk-lsp-spec.md`](../bynk-lsp-spec.md). It does **not** add a capability:
  it makes the capabilities already shipped from v0.24 (ADRs
  [0052](../decisions/0052-lsp-project-diagnostics.md),
  [0053](../decisions/0053-lsp-binding-index.md), 0055–0057, 0063–0070, 0093–0095,
  [0190](../decisions/0190-hover-resolves-references-not-just-declarations.md),
  [0191](../decisions/0191-a-renderer-arm-for-every-resolved-index-kind.md)) rest
  on a foundation that matches the compiler and the protocol.
- **Posture:** Feature track per
  [ADR 0076](../decisions/0076-feature-track-posture.md). Qualifies on **two** of
  the three axes — **multi-increment** (the project model, the freshness
  contract, the lifecycle, per-workspace state, the scheduler, and the test seam
  are each their own MINOR with its own fixtures) and **surface not yet settled**
  (the `bynk-ide` analysis API's shape, what path identity means once there is
  more than one root, and what a handler *does* on a stale round are all open).
  It is **not** a security/safety boundary — see §6.
- **Front-loaded ADRs (named, not numbered):** **file identity is not the
  unit-validation path** (slice 0 — how a multi-root project names its files
  when the tree-relative form is load-bearing elsewhere; §3.1); the **LSP
  analyses the compiler's project model** (slice A — one manifest-aware
  discovery shared by `bynkc` and the server); the **freshness contract**
  (slice B — what an index-backed handler does when the round it would answer
  from predates the buffer the client is asking about). Each is created and
  numbered by the slice that lands it (§8) — this doc deliberately does not
  pre-allocate numbers, since concurrent tracks would collide.

## 1. Motivation

The Bynk LSP's *feature* surface is, by the standard this repo set for it,
finished. Every capability the server advertises is backed by a handler; ADR
0156 has held the editor surface in step with the language for fifty
increments; ADR 0191's coverage sweep pins a renderer arm to every index kind.
An external review put it plainly: "unusually feature-complete."

The same review found four foundational gaps, and they share a shape worth
naming, because the shape — not any individual defect — is the reason to open a
track:

> **Every gap is in the transport/lifecycle layer, and every test of that layer
> asserts on static shape rather than behaviour over time.**

The layer is not untestable. `bynk-lsp/src/main.rs:2665` carries a
`#[cfg(test)] mod tests` whose thirteen tests name `Backend::find_source_root`,
`Backend::resolve_root`, and `server_capabilities()` directly, and
`cargo test -p bynk-lsp` opens with `Running unittests src/main.rs … 150
passed`. `main.rs` *is* compiled into a test binary — the bin crate's own. What
those tests assert is the point: *static* shape (does the advertisement contain
this flag? does root resolution walk up to `src/`?), never *behaviour over
time* (does a round answer against the buffer the client actually has? does a
publish carry the version it was computed from?).

The substitution was deliberate, accepted, and never revisited. The capability
tests say so in their own prose:

> the "trivial unit check" the proposal scopes in place of a transport
> round-trip — `main.rs:2836`

That is the mechanism. Not impossibility — a standing trade, taken increment
after increment, that bought a cheap assertion about *shape* in place of an
expensive one about *behaviour*. Nothing blocked the expensive one: `main.rs:2661`
already calls `LspService::new(Backend::new)`, so an in-crate `#[cfg(test)]`
harness could drive a `didChange` → `hover` round **today, with zero refactor**.
It was simply never written.

The separate constraint — real, and the reason slice C exists — is that
`bynk-lsp` declares a `[[bin]]` target and no `[lib]`
(`bynk-lsp/Cargo.toml`), so an *integration* test cannot `use bynk_lsp::…`.
The nine files in `tests/` therefore `#[path]`-include individual *modules*
(`../src/position.rs`, `../src/hover.rs`) and call their pure functions;
`tests/support/mod.rs` states the design outright ("pure functions only") and
`tests/hover_references.rs:25` concedes "`Backend::hover` is transport and
cannot be [tested]" — true of that file, not of the crate.

So: `tests/hover_references.rs` drives "the real hover ladder (`Backend::hover`'s
pure core)" over pre-analysed fixtures — it tests the half of hover that was
already correct, while the staleness defect sits in the half that fetches the
snapshot. And the 150 in-crate tests, which *can* see that half, only ever ask
it static questions.

This is not neglect. It is the predictable end state of a good instinct — push
logic into pure, testable modules, which is *why* the feature surface is
excellent — paired with a cheap substitute for testing the layer that wires
them to the protocol. The under-tested layer is exactly, and not
coincidentally, where all four gaps are.

Three forces converge on a foundations track:

1. **The LSP analyses a different project from the compiler.** `bynkc` routes
   through `CompileOptions::split(input.to_path_buf(), read_project_paths(input))`
   (`bynk-driver/src/lib.rs:24`, whose `CompileOptions::split`
   (`bynk-emit/src/project.rs:324`) builds `Roots::Split` at `:328`), honouring
   the flat `include`/`exclude` layout
   [ADR 0147](../decisions/0147-structural-test-ness-and-flat-paths.md) settled.
   The LSP reduces that manifest to a single `src_dir` string
   (`bynk-lsp/src/project.rs:187-194`, first `include` only, `exclude` ignored)
   and hands `root/src` to the analyser as if it were a project root
   (`bynk-lsp/src/main.rs:306`). Two tools, two project shapes, one manifest.
2. **The freshness contract is unwritten, so there isn't one.** `index_position`
   resolves the client's *current* cursor position against the *previous*
   round's snapshot with no version comparison (`main.rs:784-799`). Nine of ten
   position handlers take that path.
3. **The lifecycle advertises more than it implements.** Workspace folders and
   change notifications are advertised (`main.rs:2071-2075`) with one
   `project_root`, `folders.first()`, and no handler. `initialized` only logs.
   Nothing is ever dynamically registered.

Each is individually a defect. Together they are one statement: the foundation
was never specified, so it was never tested, so it drifted from both the
compiler beside it and the protocol above it.

## 2. Scope and non-goals

**In scope.**

- **Behaviour-over-time tests** — a harness that exercises the server through
  the protocol (initialize handshake, document versions, publish races,
  workspace mutation, the manifest matrix) rather than asserting on static
  shape. Writable in-crate today (§4.1); it needs no refactor to exist.
- A **testable seam** — a `[lib]` target for `bynk-lsp`, so *integration* tests
  can name the server at all and the nine `#[path]`-include files retire.
  Hygiene, not a precondition.
- **One project model** — `bynk-ide` exposes a manifest-aware, multi-root
  analysis API; the LSP consumes the compiler's discovery rather than
  re-deriving a lesser one.
- **A freshness contract** — a written, tested rule for what an index-backed
  handler does when its round predates the buffer, and a document version on
  every published diagnostic.
- **Per-workspace project state** — real multi-root support behind the
  capability already advertised, plus `did_change_workspace_folders`.
- **A startup + watcher lifecycle** — the documented startup analysis, and
  dynamic registration of `workspace/didChangeWatchedFiles` so a client that
  isn't VS Code is notified.
- **One scheduler** — a single generation-based debounce covering project and
  single-file mode.
- **Doc consolidation** — the spec's obsolete `[paths].src` schema and its
  deferred-but-shipped inventories, brought current.

**Non-goals.**

- **New capabilities.** The review's "remaining intentional feature gaps" —
  rename for local bindings, match-arm navigation, dispatch edges in call
  hierarchy, true range formatting, auto-imports, semantic-token delta, pull
  diagnostics — are real and are **not** this track. They are ordinary
  increments against `bynk-lsp-spec.md`, and several get materially easier once
  the foundation lands. A track that also grows the surface would never retire.
- **Performance work.** Semantic-token delta and pull diagnostics are
  throughput, not correctness. Deferred by the same rule.
- **Re-litigating ADR 0147.** The flat `include`/`exclude` layout is settled.
  This track makes the LSP *obey* it, not revisit it.

## 3. The core problem: two project models from one manifest

The load-bearing decision is where the project model lives, and it is worth
stating precisely how the current split arose, because the fix is mostly
*deletion*.

The compiler's discovery is already correct and already general:

- `read_project_paths(root)` (`bynk-emit/src/project/paths.rs:106`) parses
  `[paths] include`/`exclude` with the `toml` crate, falling back to
  `ProjectPaths::conventional` — which walks `src` and `tests` when they exist
  and **the project root itself when neither does**, so a flat project needs no
  config.
- `Roots::Split` resolves `(primary, secondary)` from `include[0]`/`include[1]`,
  a `tests_prefix` for the secondary tree's identity paths, and `excludes` —
  the author's list plus `out`/`node_modules`.
- `compile_project` consumes exactly that (`bynk-emit/src/project.rs:372-374`).
- The driver routes every verb through it — though *conditionally*:
  `bynk-driver/src/lib.rs:23` takes the split path only when `bynk.toml` exists
  or `src/` is a directory, and falls back to `CompileOptions::single`
  otherwise. That condition is itself input to Q1's flat-project matrix.

None of it is on the LSP's path. `analyse_project` — the analysis entry point,
and the *only* caller of the machinery from the IDE side — bypasses all of it
(**annotated**, not quoted: the argument comments below are this doc's, not the
source's):

```rust
// bynk-emit/src/project.rs:555 — arguments faithful, comments added here
pub fn analyse_project(root: &Path, overlay: &HashMap<PathBuf, String>) -> ProjectAnalysis {
    match run_checks(
        root,          // src_root
        root,          // tests_root — the same tree
        Path::new(""), // tests_prefix — none
        …
        &[],           // excludes — none
```

One root for both roles, no prefix, **no excludes**. The LSP then compounds it:
`project.rs` reduces the manifest to `src_dir = include[0]` (defaulting to the
string `"src"`, where the compiler would default to *conventional*), and
`main.rs:306` passes `root/src` as the `root` argument — so `analyse_project`
looks for `root/src/bynk.toml`, finds nothing, and defaults again.

The observable consequences, in the review's words and confirmed at the source:

| Consequence | Why |
|---|---|
| Secondary roots (`tests/`) invisible | `include[1]` — precisely `Roots::resolve`'s secondary — is dropped |
| Excluded/generated sources analysed | `excludes: &[]`; `out`/`node_modules` not skipped either |
| Manifest-backed flat projects fail | LSP defaults to `"src"`; compiler defaults to `include = ["."]` |
| Diagnostics/references/rename disagree with `bynkc` | Different file sets, by construction |

`examples/todo` is the in-repository proof: its `bynk.toml` declares no
`[paths]` at all, so `examples/todo/tests/todos.bynk` — a real `suite` the
compiler compiles — is invisible to the server. It is a ready-made regression
fixture, and it is already the file two LSP tests read for other purposes.

**The shape of the fix is reuse, not construction.** `analyse_project` should
resolve `Roots` the way `compile_project` does, and the LSP should pass the true
project root. `analyse_project` has exactly one caller
(`bynk-ide::diagnose_project`), and no wasm or in-browser caller touches it (the
REPL enters `run_checks` through its in-memory path instead).

**`diagnose_project`, though, is a wide surface — an earlier draft of this doc
said "three" and was wrong.** It has ~50 call sites across three crates, and is
public API of **both** `bynk-ide` **and** `bynkc` (re-exported at
`bynkc/src/lib.rs:47`): 30 in `bynkc/tests`, 13 in `bynk-lsp/tests`, 7 in
`bynk-lsp/src` — the last including all three of the LSP's own rounds
(`main.rs:308` the debounced round, `:632` completion's receiver typing,
`:1878` rename's post-check). Nearly all the test callers hand in a fixture
root and want exactly today's single-tree behaviour, which is why Q1's
"convenience for callers that genuinely want one tree" is the load-bearing half
of that question rather than an afterthought.

**The cost is not discovery — it is identity.** Today every path in `Analysis`
is relative to `src_root`, a single directory. Once there are two roots, a
project-relative identity is the only thing that is well-defined, which moves:
`Analysis.src_root` (`main.rs`), `uri_to_rel` (`main.rs:658-665`, a single
`strip_prefix`), the `versions` map keys (`main.rs:297`), the `abs` rebuild on
publish (`main.rs:321`), and every fixture that hardcodes an `src`-relative
path.

### 3.1 Identity is ambiguous, not merely relative (slice 0)

That cascade is not the whole cost, because **the identity a multi-root project
would cascade to does not exist yet**. This is a compiler-layer defect that
predates the track; it was found while writing slice A's proposal (#647) and it
blocks that slice.

`parse_tree` (`bynk-emit/src/project.rs:781`) computes each file's
`source_path` by stripping **its own tree's root**:

```rust
let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();
…
snapshots.push((rel.clone(), source.clone()));
```

`parse_tree` is called once per tree — `parse_tree(src_root, …)` then, in split
mode, `parse_tree(tests_root, …)`. So `root/src/todos.bynk` and
`root/tests/todos.bynk` both yield `todos.bynk`. `Roots::tests_prefix` does
**not** fix this: it never reaches `snapshots`, and is applied downstream in
`tests_emit.rs:2890` for emitted test paths only.

Measured on `examples/todo` (the track's own regression fixture), via a
roots-aware analyse over `Roots::Split`:

```
include = ["src", "tests"]  exclude = []
SNAPSHOT KEYS = ["todos.bynk", "todos.bynk"]
count=2 unique=1
```

`src/todos.bynk` declares `context todos`; `tests/todos.bynk` declares
`suite todos`. Two files, one key. Downstream, `bynk-ide::diagnose_project`
does `by_file.remove(&source_path)` per snapshot, so the first entry drains
*both* files' diagnostics and the second gets none; the LSP's
`snapshots: HashMap` clobbers one outright. `ProjectAnalysis.snapshots`'
doc comment already claims "project-relative source path" — it is not, and that
false comment is the likeliest reason this went unnoticed.

**Why the obvious repairs fail.** `source_path` serves two masters that only
agree while there is one root:

- *Make it project-relative everywhere.* `check_path_name_alignment`
  (`consistency.rs:90`) requires the tree-relative form — `src/todos.bynk`
  declaring `context todos` matches only because rel is `todos.bynk`.
  Project-relative gives `["src","todos"] ≠ ["todos"]` and every unit in the
  project fails validation.
- *Prefix only the secondary tree.* `consistency.rs:83` exempts
  `UnitKind::Test | Integration` from alignment, so this works for a `tests/`
  tree of suites — but [ADR 0147](../decisions/0147-structural-test-ness-and-flat-paths.md)
  made test-ness **structural, not directory-based**, so `include[1]` may
  legally hold a non-test unit, which would then fail alignment. Correct for
  `examples/todo`, wrong for a layout the ADR explicitly permits.

So identity had to be **separated from the unit-validation path** rather than
redefined: unit validation keeps the tree-relative form, and the analysis result
gained an unambiguous project-relative key (`ParsedFile::identity_path`).
[ADR 0198](../decisions/0198-file-identity-is-not-the-unit-validation-path.md)
settles it.

Two corrections review surfaced, recorded because each is this track's own
failure mode aimed at itself:

- **"Unaffected by construction" was narrower than it sounded** — an earlier
  draft of this section said it, and it was false. `Roots::Single` is not "one
  `include` root": `bynk-driver` selects `Roots::Split` whenever a `bynk.toml`
  exists *or* `src/` is a directory, so a conventional `src/`-only project has
  one root and **still** re-bases (`math.bynk` → `src/math.bynk`). Untouched are
  `Roots::Single` (no manifest **and** no `src/`), in-memory builds, and the flat
  layout (`include = ["."]`, normalised to an empty prefix — `.` only *looks*
  like a join identity).
- **The green suite proved nothing.** Thirteen fixtures re-base and none
  observes it: `expected_error.txt` asserts category strings, never paths. The
  churn the proposal budgeted for was absent for precisely the reason the defect
  survived sixty increments. The coverage hole and the bug have one shape — which
  is why slice 0's tests are in-crate and mutation-checked.

## 4. Internal architecture

### 4.1 The test seam (and what it is *not* a precondition for)

Two distinct things get conflated here, and the distinction decided Q5.

**The harness needs no seam.** A behaviour-over-time test — `didChange` bumps
the version, then `hover` must not answer from the old snapshot — can be
written **today**, in-crate, with zero refactor: `main.rs:2661` already calls
`LspService::new(Backend::new)`, which hands back the `Client` that `Backend`
needs, and the existing `#[cfg(test)] mod tests` (`main.rs:2665`) is already
the place to put it. Every slice below can therefore carry protocol-level
regression evidence **without** the seam landing first. Any claim otherwise —
including this doc's earlier drafts — is false.

**The seam is hygiene, and hygiene is still worth a slice.** Extracting
`Backend`, the state struct, and the `LanguageServer` impl into `src/lib.rs`,
leaving `main.rs` a thin `fn main()`, buys three things the in-crate harness
does not: *integration* tests can `use bynk_lsp::…` at all (today they cannot —
the crate has no `[lib]`); the nine `#[path]`-include files in `tests/` retire
a workaround that exists only because of that; and the transport tests stop
having to live inside the binary they test. Note the `exclude` list in
`bynk-lsp/Cargo.toml` (tests reading sibling directories, kept out of the
published tarball) is orthogonal and survives unchanged — though it is prior
art for Q6's packaged-crate constraint.

What the harness asserts, wherever it lives: that `initialize` advertises what
the server implements, that a version-bumping `didChange` makes a subsequent
hover decline or refresh rather than answer from the old snapshot, that a
publish carries the version it was computed from, that
`did_change_workspace_folders` re-roots analysis. In-process or spawned over
real `Content-Length` framing was **Q6**, now settled: in-process, not
re-tested in `cargo test` (the VS Code CI job covers real framing). In-crate or
behind the seam was **Q5**, settled — in-crate, because slice A led and needed
no seam.

### 4.2 The freshness contract

`ensure_analysis` (`main.rs:642-648`) returns the cached round whenever one
exists and re-analyses only on cold start; `fresh_analysis` (`652-655`)
unconditionally re-analyses. Exactly one handler — `rename` (`main.rs:1857`) —
passes `fresh: true`. The other nine position handlers take the cached round:

| Handler | line | source |
|---|---|---|
| `rename` | 1857 | `index_position(…, **true**)` — fresh |
| `hover` | 965 | cached |
| `prepare_call_hierarchy` | 1159 | cached |
| `goto_implementation` | 1224 | cached |
| `goto_type_definition` | 1254 | cached |
| `goto_definition` | 1460 | cached |
| `references` | 1616 | cached |
| `document_highlight` | 1789 | cached |
| `prepare_rename` | 1830 | cached |
| `symbol` | 1758 | `ensure_analysis()` |

`Analysis.versions` already exists and is already populated (`main.rs:297`) —
but it is read in only two places (`main.rs:1677`, `main.rs:1931`), both of which
*stamp outgoing edits* so the client can reject them. The server never checks it
on the way in. Read-only handlers have no such backstop: an edit that adds a line
above the cursor silently shifts the resolution, and hover answers about the
wrong symbol with no signal to anyone.

**Seven** handlers are worse than the review states: they read `state.analysis`
**directly**, bypassing `ensure_analysis` entirely, so they return empty on cold
start rather than triggering a round. Slice B must cover all seven — an earlier
draft of this doc named only the first three, which would have left `code_lens`
(the one carrying *both* defects) untouched:

| Handler | line | exposure beyond cold start |
|---|---|---|
| `code_action` | 1652 | — |
| `inlay_hint` | 1688 | — |
| `semantic_tokens_for` | 746 | backs both the full and range requests |
| `code_lens` | 1068 | **also resolves text from `analysis.snapshots`** — the same staleness shape as `index_position` |
| `document_link` | 1289 | no backstop |
| `incoming_calls` | 1174 | weaker — follows a `prepare` that ensured |
| `outgoing_calls` | 1196 | weaker — as above |

Two are unaffected and should stay that way: `completion` (1318) and
`document_symbol` (1570) read live text from `state.docs`.

What a handler *does* on a mismatch is **Q3**, settled in §7: **refresh** — run
a round so the answer resolves against the buffer the client has, never serve a
position against a stale snapshot. Uniform across the index-backed handlers;
`completion`/`document_symbol` stay outside it (they read live text). Still a
front-loaded ADR rather than a bug fix, because it is a written contract over
ten handlers and a visible behaviour commitment, not a local patch.

### 4.3 Per-workspace state

The capability is advertised (`main.rs:2071-2075`: `supported: true`,
`change_notifications: true`); the implementation is one `Option<PathBuf>`
(`main.rs:118`), populated from `folders.first()` (`main.rs:846-848`), with no
`did_change_workspace_folders` handler anywhere in the crate. Additional folders
are silently ignored.

Settled by this track's scoping: **implement it**, don't withdraw it — and
**shipped in slice D** ([ADR 0204](../decisions/0204-per-workspace-project-state.md)).
State is a map — each entry its own `ProjectConfig`, `Analysis`, generation
counter, and published-diagnostics set — and every request routes by URI to its
owning entry. What that entry is keyed by, and what answers a request for a file
that has no entry, was **Q4** (settled #672): **the key is the discovered project
root, not the workspace folder.** Routing reuses `resolve_root`, which walks a
file's path up to its nearest enclosing `bynk.toml` (else `src/`, else `None`) —
the same attribution `bynkc` gives that file. Workspace folders seed *discovery*;
they do not own URIs. A file whose walk finds no root keeps single-file mode. So
the state map is keyed `project_root → entry`; `did_change_workspace_folders`
adds and prunes *discovery seeds*, not routing owners (retaining a project with
an open buffer); and a nested folder pair collapses to whatever set of
`bynk.toml` roots lies under it, ambiguity-free. `workspace/symbol` aggregates
across projects — the one cross-project query.

### 4.4 Startup and watchers

**Shipped in slice E (v0.183).** Before it, `initialized` logged on both
branches and returned: the spec documented a startup project analysis that did
not exist, so a workspace activated by `workspaceContains:bynk.toml` showed no
diagnostics until a `.bynk` file was opened. Now `initialized` warms every
project the folders hold (`discover_projects_under`, a bounded `bynk.toml`
walk), so diagnostics appear at activation, and it registers the file watchers
server-side (below).

Before slice E, `register_capability` was called nowhere: the
`did_change_watched_files` handler worked *only* because the VS Code extension
supplied the watchers client-side (`synchronize.fileEvents`), so for any other
client it was dead code. Slice E registers `**/*.bynk`/`**/bynk.toml`
server-side (dynamic registration, gated on the client capability), and the
extension **drops its client-side `fileEvents`** so the server's registration is
the single source (no double-notification). A client without dynamic
registration still supplies watchers itself, so it is never left without.

### 4.5 One scheduler

The review's claim that debouncing "does not behave as documented" is right; its
diagnosis is half wrong, and the correction matters for scoping.

`schedule_project_diagnostics` (`main.rs:259-273`) **does** implement
generation-based cancellation — it bumps `analysis_generation`, sleeps, and bails
if superseded. Project mode does not run N analyses per N keystrokes. The real
defects are narrower:

- **The debounce stacks.** `did_change` (`main.rs:939-942`) sleeps the
  configured `diagnostics_debounce_ms`, then `recompile_and_publish` (219-222)
  delegates to `schedule_project_diagnostics`, which sleeps **another
  hardcoded 200 ms** (`main.rs:267` — not from config). Effective latency is
  `diagnostics_debounce_ms + 200`.
- **Single-file mode has no scheduler at all.** `main.rs:223-239` has no
  generation check: every change's task unconditionally runs
  `bynk_ide::diagnose` and publishes. The comment at `935-937` claims changes
  "effectively coalesce because each task reads the latest text at recompile
  time" — that is not coalescing, it is redundant work converging on the same
  answer.
- **Cancellation is pre-flight only.** A burst arriving while
  `run_project_diagnostics` is already in flight does not cancel it; the
  round-committal check (`main.rs:403`) discards it at publish time instead.

One generation-based scheduler covering both modes, with the fixed 200 ms folded
into the configured value, is the whole slice.

## 5. Tooling delta (the standing rule)

[ADR 0156](../decisions/0156-editor-surface-tracks-language.md) requires every
slice to state what hover, completion, semantic tokens, and signature help do
now. This track adds no language construct, so the answer for every slice is
**"unchanged, because this track changes no surface"** — with one deliberate
exception: the freshness slice (§4.2) changes what all four *do on a stale
round*, from "answer from the old snapshot" to "refresh to the current buffer,
then answer" (Q3). That is a
behaviour change to all four and each slice proposal must say so explicitly
rather than inheriting this paragraph.

## 6. Security & threat model

**None, because** the security/safety trigger is not ticked. The LSP reads
project sources and answers a local editor over stdio; it provisions nothing,
authenticates to nothing, and has no outward-facing side effects. The one
adjacent consideration is not a security boundary but a correctness one already
named in §4.2: a stale round can cause the server to emit a *workspace edit*
against text the user no longer has. Both edit-emitting handlers already stamp
`versions` so the client rejects it — the mechanism works; this track extends
the same discipline to the read-only handlers, which have no backstop at all.

## 7. Open questions (settle before slicing)

Each question gates the slice that turns on it — not the track as a whole. Q5 is
settled below; Q1/Q2 have moved to slice A's proposal as `[DECISION]` forks.

- **Q1 — the analysis API's shape. — SETTLED by slice A ([ADR 0201](../decisions/0201-the-lsp-analyses-the-compilers-project-model.md)).**
  `bynk-ide` owns `AnalysisRoots` and grows `diagnose_project_with`;
  `diagnose_project` stays the single-tree convenience, which is what makes the
  change additive across ~50 call sites in three crates. The original question
  below, for the record: does `bynk-ide` take `Roots` directly
  (leaking a `bynk-emit` type through the IDE surface), or its own
  `AnalysisOptions` that it lowers? Does `analyse_project` keep an
  `(root, overlay)` convenience for callers that genuinely want one tree?
  *Investigation:* the ~50 `diagnose_project` call sites across `bynk-ide`,
  `bynk-lsp` and `bynkc` (§3) — the convenience is what keeps ~47 of them, and
  `bynkc`'s public API, from churning; whether `Roots` is already public API.
  **Migrated to slice A's proposal as `[DECISION A]`, where it is concrete.**
- **Q2 — path identity across roots. — RE-SCOPED to slice 0; its premise was
  false.** The question asked whether the LSP should adopt "the compiler's
  `tests_prefix` asymmetry" or normalise. **There is no asymmetry to adopt.**
  `parse_tree` (`bynk-emit/src/project.rs:781`) strips *each tree's own root*,
  and `tests_prefix` never reaches `snapshots` — it is applied downstream, in
  `tests_emit.rs:2890`, for emitted test paths only. So with two `include`
  roots the identity is not asymmetric; it is **ambiguous**. Demonstrated on
  `examples/todo` (§3.1): two files, one snapshot key.

  The real question — *how a project gets unambiguous file identity without
  breaking unit validation, which needs the tree-relative form* — is a
  compiler-layer defect that predates this track and is now **slice 0**. Q2 as
  written is withdrawn; slice 0's ADR settles it.
- **Q3 — the freshness contract (front-loaded ADR). — SETTLED: refresh,
  uniform; never serve stale, decline only when no round is possible.** On
  `version != versions[rel]`, an index-backed handler catches up — runs a round
  so its answer resolves against the buffer the client actually has — before
  answering. Never returns a position resolved against a stale snapshot. This is
  the front-loaded ADR slice B lands; the decision is settled here so B's
  proposal can be cut.

  *Why refresh, not decline — the measurement first, since the doc required it.*
  A full project round (release, `diagnose_project_with` over `examples/todo`'s
  layout and synthetic projects):

  | files | round | files | round |
  |---|---|---|---|
  | `examples/todo` (2) | 1.9 ms | 100 | 26 ms |
  | 10 | 1.0 ms | 200 | 91 ms |
  | 50 | 8.5 ms | | |

  Sub-10 ms for any realistic Bynk project, and under the ~100 ms "responsive"
  threshold even at 200 files — an order of magnitude larger than the biggest
  example in the tree. The "refresh adds latency to every keystroke-adjacent
  hover" worry the first draft of this question raised is real in principle and
  empirically small at Bynk's scale: the latency it warns of is a few
  milliseconds. **Decline** trades that away for a worse experience — hover
  flickering to empty *while the user reads it* — to save single-digit
  milliseconds. Not a trade worth making.

  *Prior art.* rust-analyzer and gopls both gate requests on a snapshot/revision
  at or after the edit and **block until current** — neither serves position
  data against a stale snapshot, neither declines. They afford it because they
  are *incremental* (Salsa / gopls's snapshot graph); a request refreshes only
  what the query touches. **Bynk has no incremental layer** — every round is
  whole-project — so a refresh here is the full cost above, and it scales with
  project size where theirs stays flat. That is fine now (measured) and is the
  one thing that would change the answer if real Bynk projects ever grew to
  many hundreds of files. Named as the scaling cliff, not a surprise.

  *Uniform, not per-handler.* Every index-backed handler refreshes on mismatch.
  Per-handler policy (stale `code_lens` counts tolerable, stale `hover` not) is
  complexity without a demonstrated need, and prior art is uniform. Two handlers
  stay outside the contract entirely because they already sidestep the index:
  `completion` and `document_symbol` read **live text** from `state.docs`.

  *Decline survives only for the genuinely unanswerable* — single-file mode (no
  project), a file outside every `include` root, a round that bailed with no
  snapshots. Those return `None` today and continue to. Decline is never the
  answer to *staleness*.

  *One implementation note for slice B (not a decision, a hazard).* The refresh
  must **coalesce with the debounced round**, not spawn a redundant parallel
  one. `did_change` already schedules a round (§4.5), and `fresh_analysis`
  (`main.rs:652`) today spawns unconditionally — so a naive per-request refresh
  plus the debounce runs the whole-project analysis twice. Slice B should
  await-or-spawn keyed on the requested version (the round machinery already
  carries `analysis_round_started`/`committed` and a generation counter to build
  on), so a hover during active typing rides the round the keystroke already
  triggered rather than racing a second one.
- **Q4 — request routing under multiple folders. — SETTLED: route by the
  discovered project root, not the workspace folder; a file under no project
  stays single-file; one global `didChangeWatchedFiles` registration.** The
  three sub-questions, and why each answer is the grounded one:

  *Which folder owns a URI when folders nest? — none; the project does.* The
  premise mis-locates ownership. A file already has a well-defined owner and the
  server already computes it: `resolve_root` (`main.rs:212`) walks the file's
  path **upward to the nearest enclosing `bynk.toml`** (else the nearest `src/`,
  else `None`) — which is exactly the project `bynkc` attributes that file to.
  So routing is `resolve_root(uri) → project_root`, and the state map is keyed by
  that root, not by workspace folder. Nesting then has no tie to break:
  overlapping or nested folders resolve to whatever set of `bynk.toml` roots
  lies beneath them, and each file lands in its nearest one. Two folders sharing
  one root share one project entry (not two); one folder holding two roots yields
  two entries. Workspace folders are **discovery seeds** — where the server looks
  for roots on startup (slice E) and on `did_change_workspace_folders` — never
  the routing key. This keeps the track's core invariant: the LSP agrees with
  `bynkc` about which project a file is in, by using the *same* walk-up rule
  rather than a second, folder-shaped one.

  *What answers a request for a file under no project? — the single-file path,
  not nothing.* When `resolve_root` returns `None` (no ancestor `bynk.toml`, no
  `src/`), the file analyses as a lone tree — `bynk_ide::diagnose` over the open
  buffer, `Roots::Single` for index-backed handlers. This is not a new decision;
  it is what the server does **today** (`main.rs:233-253`: no project root ⇒ the
  per-buffer path) and what slice A settled for the rootless tree. Declining to
  *nothing* would regress a file the user can open and get diagnostics on right
  now. Decline stays reserved for the genuinely unanswerable (Q3's rule), never
  for "outside every folder."

  *One `didChangeWatchedFiles` registration, or one per folder? — one global.*
  The patterns Bynk needs are folder-independent — `**/*.bynk` and `**/bynk.toml`
  — so a single dynamic registration with those two globs covers every folder the
  client holds, and a file event carries an absolute URI that routes through
  `resolve_root` regardless of which registration matched. This mirrors what VS
  Code supplies client-side today (§4.4: one `createFileSystemWatcher("**/*.bynk")`,
  not one per folder). `did_change_workspace_folders` therefore does **not**
  re-register watchers — the glob set does not depend on the folder list — which
  avoids an unregister/register churn on every folder change.

  *Prior art, honestly.* rust-analyzer and gopls both key analysis on the
  **manifest**, not the editor's folder list — a crate/module discovered by
  walking to `Cargo.toml`/`go.mod`, with a file routed to its narrowest
  enclosing one and a standalone view for files outside every manifest
  (rust-analyzer's *detached file*, gopls's ad-hoc `command-line-arguments`
  package). Q4 lands in the same place: manifest is the unit, nearest-enclosing
  is the route, single-file is the fallback. The one
  place Bynk diverges is watcher registration: rust-analyzer registers *per-project*
  watchers with relative patterns **because** it watches directories outside the
  workspace (sysroot, the registry cache). Bynk has no out-of-tree sources — every
  `.bynk` file of interest is under a workspace folder — so the reason for
  per-folder registration does not apply, and one global glob is both sufficient
  and simpler.

  *Consequences for slice D (recorded, decided at implementation).* Keying by
  project root, not folder, means `did_change_workspace_folders` **added** folders
  are scanned for roots and spun up (proactive, so diagnostics appear before a
  file is opened — the startup-scan machinery slice E also uses); **removed**
  folders drop the entries for roots no longer under any remaining folder — except
  an entry still holding open documents, which routing needs and which is retained
  until its last buffer closes. This per-workspace project model — the protocol's
  multi-root capability mapped onto Bynk's manifest-discovered projects — is a
  durable architectural commitment, so **slice D lands an ADR** for it, numbered
  at merge (it is not one of the header's front-loaded ADRs; it was identified
  here, at the settling of the question it turns on).
- **Q5 — slice ordering: seam or model first? — SETTLED: model first.**
  Slice A leads; the seam (slice C) lands on its own merits whenever, and is
  **not** a precondition for anything.

  *How it was settled.* This doc's first draft recommended seam-first on the
  argument that the model slice needs transport-level regression evidence and
  could not otherwise have any. **That argument was false.**
  `LspService::new(Backend::new)` (`main.rs:2661`) means an in-crate
  `#[cfg(test)]` harness can drive a `didChange` → `hover` round today, with no
  refactor at all (§4.1) — so slice A can carry its own behaviour-over-time
  evidence with the seam nowhere in sight. With the necessity claim withdrawn,
  what remained for seam-first was hygiene, which is real but is not a reason
  to delay the one gap users can actually observe: the LSP disagreeing with
  `bynkc` about which files exist.

  *The case against, which was weighed and is not free.* Slice A moves path
  identity through the nine `#[path]`-including test files; doing that churn
  once — after the seam — would be cheaper than doing it twice. **Accepted
  cost:** the churn is mechanical (a `strip_prefix` base and fixture paths),
  it is bounded by those nine files, and paying it twice is a smaller price
  than shipping a known-wrong project model for another two increments while a
  refactor lands. Time-to-value wins; the estimate is recorded here so a later
  slice C can be judged against it rather than re-arguing the ordering.

  *Consequence for §8:* the lettering is now a landing order for A and C, not
  merely a dependency map. Q1 and Q2 — slice A's own questions — migrate to
  that slice's proposal as `[DECISION]` forks, which is where the increment
  template puts sign-off points and where they are concrete rather than
  hypothetical.
- **Q6 — harness depth. — SETTLED: in-process. Real `Content-Length` framing
  is not re-tested in `cargo test`; the VS Code CI job already spawns the real
  binary end-to-end.** Slice C's harness drives the server in-process — the
  behaviour-over-time tests slices A and B already wrote (`LspService::new(
  Backend::new)`, driving `Backend`), which the seam only *moves* from the
  in-crate `#[cfg(test)]` module into `tests/` as integration tests over `use
  bynk_lsp::…`. The spawned-binary variant is **deferred, not rejected**.

  *Why in-process is the right depth, not a compromise.* The framing layer is
  `tower-lsp`'s codec plus `main.rs`'s ~two-line serve loop (`Server::new(stdin,
  stdout, socket).serve(service)`, `main.rs:2863-2864`). The codec is
  third-party and tested; the serve loop is wiring, not Bynk's logic. A
  spawned-binary `cargo test` would spend a process spawn and a build-dependency
  to test code Bynk does not own — and the repo **already** has that coverage:
  the VS Code integration CI job spawns the real `bynkc-lsp` and drives real
  requests against a fixture workspace, which is the authoritative end-to-end
  framing/wiring guarantee (and, until slice A, was the *only* end-to-end LSP
  coverage). A second spawned harness in `cargo test` duplicates it at real cost
  for a layer that rarely breaks.

  *What in-process does and does not cover.* It covers every handler's
  behaviour, the state machine, the freshness gate, the round machinery — all of
  Bynk's own code, driven through the real `Backend` over a real `LspService`.
  It does **not** cover `Content-Length` framing or the stdio serve loop; those
  are the VS Code job's. That division is deliberate: `cargo test` stays fast and
  focused on Bynk's code; the slow, real-transport check lives in the one CI job
  built for it.

  *Deferred, with a trigger.* If a serve-loop or framing regression ever reaches
  a user — something the VS Code job would catch but a developer wants to catch
  locally in `cargo test` — add one spawned-binary smoke test then, gated out of
  the published tarball via the `exclude` precedent (`env!("CARGO_BIN_EXE_bynkc-lsp")`
  gives the path; the sibling-reading tests already listed there are the
  pattern). Not now: it is cost without a demonstrated gap.

## 8. Slice decomposition (ordered)

**Q5 settled: A leads** — of the *LSP* slices. **Slice 0 precedes it**: a
compiler-layer prerequisite found while writing A's proposal (§3.1), which A
cannot be built on top of. Structural dependencies are now `A after 0` and
`D after A`. B, E and F remain independent.

- **Slice 0 — file identity in `ProjectAnalysis`.** ✅ *shipped v0.175,
  [ADR 0198](../decisions/0198-file-identity-is-not-the-unit-validation-path.md),
  #650.* **Compiler-layer, not LSP.** A
  project's analysis result gains an unambiguous project-relative identity,
  separate from the tree-relative `source_path` that
  `check_path_name_alignment` needs (§3.1 shows why neither can simply become
  the other). Scope: `bynk-emit`'s `parse_tree`/`ProjectAnalysis` and the error
  attribution keyed on it, plus the `snapshots` doc comment that currently
  claims an identity it does not provide. `Roots::Single` is unaffected by
  construction (`(root, root)` ⇒ project-relative ≡ tree-relative), so every
  existing single-tree caller is untouched; the churn is `expected_error.txt`
  paths for split projects, which is compiler-visible and is this slice's to
  own rather than A's to smuggle. Testable through `compile_project` with
  `Roots::Split`, which exists today — it does not depend on any LSP change.
- **Slice A — one project model.** ✅ *shipped v0.178,
  [ADR 0201](../decisions/0201-the-lsp-analyses-the-compilers-project-model.md),
  #647.* `bynk-ide` exposes the
  manifest-aware multi-root API (Q1); `analyse_project` resolves `Roots` like
  `compile_project`; the LSP passes the true root and adopts slice 0's identity.
  Regression fixture: `examples/todo/tests/todos.bynk` resolves. Harness
  fixture: the manifest path matrix (flat project, two `include` roots,
  `exclude` honoured, and the `bynk-driver/src/lib.rs:23` fallback condition).
  Carries its own behaviour-over-time evidence via the in-crate harness (§4.1) —
  it does **not** depend on the seam.
- **Slice B — the freshness contract.** ✅ *shipped v0.179,
  [ADR 0202](../decisions/0202-the-freshness-contract.md), #665.* Q3 implemented uniformly across the ten position handlers; **all
  seven** direct `state.analysis` readers (§4.2's table) brought onto the same
  path — including `code_lens`, which carries the staleness defect as well as
  the cold-start one; diagnostics published with the captured version,
  re-checked immediately before publish. Explicit ADR 0156 delta (§5).
- **Slice C — the seam.** ✅ *shipped v0.180, #669 (no ADR — a refactor).*
  `[lib]` target; `Backend`/state/impl to `src/lib.rs`;
  `main.rs` reduced to `fn main()`; the nine `#[path]`-using test files migrated
  to `use bynk_lsp::…` (which also removed their redundant re-runs of module unit
  tests); transport tests moved out of the binary they test. No behaviour
  change. *No ADR* — a refactor settles nothing. **Hygiene, not a precondition**
  (§4.1). Per Q5 it trails slice A; it is otherwise unblocked and may land
  whenever.
- **Slice D — per-workspace state.** ✅ *shipped v0.182,
  [ADR 0204](../decisions/0204-per-workspace-project-state.md), #673.* A
  **project-root-keyed** state map (Q4: keyed by the discovered root, not the
  workspace folder); URI routing via `resolve_root`'s existing walk-up;
  `did_change_workspace_folders` adding and pruning discovery seeds (retaining a
  project with an open buffer); a file under no project staying single-file;
  `workspace/symbol` aggregating across projects; the advertised capability made
  true. Routing concentrates in the two funnels slice B built (`analysis_for`,
  `analysis_covering_open_buffers`) — no handler body changed. The load-bearing
  detail: the map key, `Analysis.project_root`, and routing all canonicalise, or
  a symlinked workspace path routes to a different key than the round filled.
- **Slice E — startup & watchers.** ✅ *shipped v0.183, #676 (no ADR — settled
  direction).* The documented startup analysis in `initialized`: a bounded
  `bynk.toml` tree-walk (`discover_projects_under`, the "one tree-walk" ADR 0204
  §C named) warms every project under the folders — reused for added folders and
  workspace-symbol seeding (closing D's nested-monorepo gap). Dynamic
  `register_capability` for `workspace/didChangeWatchedFiles`, gated on the
  client capability captured at `initialize`; the VS Code extension **drops its
  client-side `synchronize.fileEvents`** so the server's registration is the one
  source (no double-notification). The registration call is validated by the
  VS Code integration CI job (not observable in `cargo test`).
- **Slice F — one scheduler.** A single generation-based scheduler over both
  modes; the hardcoded 200 ms folded into the configured debounce; in-flight
  supersession (§4.5) settled.
- **Slice G — doc consolidation.** `bynk-lsp-spec.md` §2.2's `[paths].src`
  schema (`:59-60`), the site's `[paths].src` claim
  (`site/src/content/docs/docs/tooling/bynk-lsp.md:112`), and the rustdoc at
  `bynk-lsp/src/main.rs:409` that is wrong against its own body; the
  deferred-but-shipped inventories at `:29`, `:615`, `:754` — including §"Out of
  scope"'s self-contradiction with `:25-27` four lines above; §4.3's declared
  list, missing nine shipped capabilities. Also
  `../bynk-tooling-roadmap.md` §1, whose current-state list closes with
  "**workspace folders**" — true of the *advertisement* and not of the
  implementation until slice D lands, which is the drift this track exists to
  end. Docs-only: **no version bump**, no tag (`../proposals/README.md`). Rides
  last so it describes the end state.

## 9. Risks

- **Slice 0 changes compiler-visible error paths.** Split projects' diagnostics
  move from `todos.bynk` to `src/todos.bynk`, churning `expected_error.txt`
  fixtures. Mitigation: it is arguably a fix (the paths become unambiguous and
  resolve from the project root), `Roots::Single` is unaffected so the vast
  majority of fixtures do not move, and the change is owned by a slice whose ADR
  argues it rather than riding in on an LSP increment.
- **Slice A's cascade is wider than its diff looks.** Path identity touches
  every `Analysis` consumer and every fixture with a hardcoded `src`-relative
  path. Mitigation: the in-crate harness (§4.1), which needs no seam, plus
  `declaration_spans`/`hover_references`, which already read real
  `diagnose_project` output. Per Q5 the seam does **not** absorb this churn
  first — paying it twice is the accepted cost of leading with the model.
- **Q3 is settled by measurement (§7), not left open.** Refresh costs a full
  round on the request path — but that is <10 ms for any realistic Bynk project
  and <100 ms at 200 files, so the latency the risk once feared is single-digit
  milliseconds at Bynk's scale. The residual risk moves to *scaling*: no
  incremental layer means the cost grows with project size, so a future
  many-hundred-file project would reopen this. That is a named cliff, not a
  present hazard.
- **Slice D multiplies whatever slice A lands.** Ordering is load-bearing; a
  project-root-keyed map (Q4) over the wrong project model doubles the rework.
  This is the track's only forced ordering.
- **The `bynk-ide` API is public.** It is a published crate; changing
  `diagnose_project`'s signature is a breaking change for any external consumer.
  Pre-1.0 this is cheap, but the slice must say so.
- **A refactor slice with no behaviour change is easy to under-review.** Slice C
  moves the entire server implementation between files. Mitigation: land it
  against an existing harness (in-crate or migrated), so the diff is a move
  with tests either side of it, not a move on trust.

## 10. Relationship to the north star

[ADR 0156](../decisions/0156-editor-surface-tracks-language.md) opens by naming
what the editor surface *is*: "a **projection of the language** — what the
checker understands should be legible in hover, offerable in completion, and
reachable through the UI." It made that a mechanical rule, and the rule worked:
the surface has tracked the language, increment for increment, ever since.

This track is the same argument one layer down. A hover that resolves against a
snapshot the user has already edited past, a rename that cannot see the `tests/`
tree the compiler compiles, a diagnostic the client cannot place — these do not
fail loudly. They erode the thing the editor surface exists to create, which is
*trust that the tool knows what the user knows*. Fifty increments of surface
work rest on a foundation that four ADRs' worth of features assumed and none
specified.

The end state at retirement: the LSP analyses exactly the project `bynkc`
compiles, answers only from rounds it can prove current, implements exactly what
it advertises, and — the durable part — has a seam that made all four testable,
so the next fifty increments cannot quietly undo them.
