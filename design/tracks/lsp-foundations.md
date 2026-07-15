# LSP foundations — the project model, the freshness contract, and the lifecycle the feature surface rests on

- **Status:** Adopted — direction settled by the merge of the settling PR
  (#641). Adoption is **not** build authorisation: the §7 open questions are not
  yet closed, they continue settling via reviewed PRs against this doc, and
  **no slice is authorised** until the questions it turns on are answered (Q5
  gates the first one). Live state on the track's **spine issue**,
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
- **Front-loaded ADRs (named, not numbered):** the **LSP analyses the compiler's
  project model** (one manifest-aware discovery, one path identity, shared by
  `bynkc` and the server); the **freshness contract** (what an index-backed
  handler does when the round it would answer from predates the buffer the
  client is asking about). Each is created and numbered by the slice that lands
  it (§8) — this doc deliberately does not pre-allocate numbers, since
  concurrent tracks would collide.

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
project root. The blast radius is small — `analyse_project` has exactly one
caller (`bynk-ide::diagnose_project`), which has three (the LSP, plus
`tests/rename_validation.rs:46` and `tests/declaration_spans.rs:44`); no wasm
or in-browser caller touches it (the REPL enters `run_checks` through its
in-memory path instead).

**The cost is not discovery — it is identity.** Today every path in `Analysis`
is relative to `src_root`, a single directory. Once there are two roots, a
project-relative identity is the only thing that is well-defined, which moves:
`Analysis.src_root` (`main.rs`), `uri_to_rel` (`main.rs:658-665`, a single
`strip_prefix`), the `versions` map keys (`main.rs:297`), the `abs` rebuild on
publish (`main.rs:321`), and every fixture that hardcodes an `src`-relative
path. That cascade — not the manifest parsing — is the slice.

## 4. Internal architecture

### 4.1 The test seam (and what it is *not* a precondition for)

Two distinct things get conflated here, and the distinction decides Q5.

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
real `Content-Length` framing is **Q6**; in-crate or behind the seam is **Q5**.

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

What a handler *does* on a mismatch is **Q3** — the genuinely open question, and
the reason this is a front-loaded ADR rather than a bug fix.

### 4.3 Per-workspace state

The capability is advertised (`main.rs:2071-2075`: `supported: true`,
`change_notifications: true`); the implementation is one `Option<PathBuf>`
(`main.rs:118`), populated from `folders.first()` (`main.rs:846-848`), with no
`did_change_workspace_folders` handler anywhere in the crate. Additional folders
are silently ignored.

Settled by this track's scoping: **implement it**, don't withdraw it. That makes
state a map keyed by folder — each entry its own `ProjectConfig`, `Analysis`,
generation counter, and published-diagnostics set — and makes every request
route by URI to its owning folder. Routing, and what happens to a file under no
folder, is **Q4**.

### 4.4 Startup and watchers

`initialized` (`main.rs:865-885`) logs on both branches and returns. The spec
documents a startup project analysis; there isn't one. A workspace activated by
`workspaceContains:bynk.toml` therefore shows no diagnostics until a `.bynk`
file is opened or an analysis-backed request arrives.

`register_capability` is called nowhere in the crate. A `did_change_watched_files`
handler *does* exist (`main.rs:1943`) — it works only because the VS Code
extension supplies the watchers client-side:

```ts
// vscode-bynk/src/extension.ts:141-151
synchronize: { fileEvents: [
  vscode.workspace.createFileSystemWatcher("**/*.bynk"),
  vscode.workspace.createFileSystemWatcher("**/bynk.toml"),
] }
```

For any other client, that handler is dead code.

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
round*, from "answer from the old snapshot" to whatever Q3 settles. That is a
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

- **Q1 — the analysis API's shape.** Does `bynk-ide` take `Roots` directly
  (leaking a `bynk-emit` type through the IDE surface), or its own
  `AnalysisOptions` that it lowers? Does `analyse_project` keep an
  `(root, overlay)` convenience for callers that genuinely want one tree?
  *Investigation:* the three call sites; whether `Roots` is already public API.
- **Q2 — path identity across roots.** Project-relative is the only
  well-defined identity once `include` has two entries — but `Roots::tests_prefix`
  exists precisely because the compiler prefixes the *secondary* tree's paths
  and not the primary's. Does the LSP adopt that asymmetry verbatim (identical
  to `bynkc`, slightly surprising) or normalise both to project-relative
  (cleaner, divergent)? *Prior art:* `tests_prefix`'s `--format json`
  click-through rationale.
- **Q3 — the freshness contract (front-loaded ADR).** On `version != versions[rel]`:
  **refresh** (await a fresh round — correct, adds latency to every keystroke-adjacent
  hover) or **decline** (return `None` — instant, and the editor shows nothing where
  it used to show something wrong). Per-handler or uniform? Note `rename` already
  chose refresh and `completion`/`document_symbol` sidestep it by reading live
  text. *Investigation:* what rust-analyzer and gopls do; measure a real round on
  `examples/todo` before assuming refresh is affordable.
- **Q4 — request routing under multiple folders.** Which folder owns a URI when
  folders nest? What answers a request for a file under *no* folder — the
  single-file path, or nothing? Does one `didChangeWatchedFiles` registration
  cover all folders or one per folder?
- **Q5 — slice ordering: seam or model first?** *Recommendation: **model
  first** — the external review's original position.* This doc's first draft
  recommended the seam, on the argument that the model slice needs
  transport-level regression evidence and could not otherwise have any. **That
  argument was false** and is withdrawn: `LspService::new(Backend::new)`
  (`main.rs:2661`) means an in-crate `#[cfg(test)]` harness can drive a
  `didChange` → `hover` round today, with no refactor at all (§4.1). With the
  necessity claim gone, what remains for seam-first is hygiene — real, but not
  a reason to delay the one gap users can actually observe (the LSP disagreeing
  with `bynkc` about which files exist). *The honest case against:* slice A
  moves path identity through the nine `#[path]`-including test files, and
  doing that churn once — after the seam — is cheaper than doing it twice. Weigh
  fixture churn against time-to-value; both are legitimate, neither is
  structural. It is the one ordering question the whole decomposition turns on,
  and it is now genuinely open rather than settled by a false premise.
- **Q6 — harness depth.** In-process `LspService` with a scripted client (fast,
  no framing coverage) or a spawned binary over real `Content-Length` framing
  (true end-to-end, slower, and a `cargo test` that depends on a built binary)?
  *Note:* the packaged-crate constraint in `bynk-lsp/Cargo.toml`'s `exclude`
  list is precedent for tests that must not run from the published tarball.

## 8. Slice decomposition (ordered)

Numbered by dependency, not by the order they must land: **Q5 decides whether
the seam goes first or whenever.** The dependency that *is* structural is slice
D after slice A; nothing else here is forced.

- **Slice A — one project model.** *(front-loaded ADR: "the LSP analyses the
  compiler's project model")* `bynk-ide` exposes the manifest-aware multi-root
  API (Q1); `analyse_project` resolves `Roots` like `compile_project`; the LSP
  passes the true root; identity moves project-relative (Q2). Regression
  fixture: `examples/todo/tests/todos.bynk` resolves. Harness fixture: the
  manifest path matrix (flat project, two `include` roots, `exclude` honoured,
  and the `bynk-driver/src/lib.rs:23` fallback condition). Carries its own
  behaviour-over-time evidence via the in-crate harness (§4.1) — it does **not**
  depend on the seam.
- **Slice B — the freshness contract.** *(front-loaded ADR: "the freshness
  contract")* Q3 implemented uniformly across the ten position handlers; **all
  seven** direct `state.analysis` readers (§4.2's table) brought onto the same
  path — including `code_lens`, which carries the staleness defect as well as
  the cold-start one; diagnostics published with the captured version,
  re-checked immediately before publish. Explicit ADR 0156 delta (§5).
- **Slice C — the seam.** `[lib]` target; `Backend`/state/impl to `src/lib.rs`;
  `main.rs` reduced to `fn main()`; the nine test files migrated off `#[path]`
  includes; transport tests moved out of the binary they test. No behaviour
  change. *No ADR* — a refactor settles nothing. **Hygiene, not a precondition**
  (§4.1); Q5 decides whether it leads or trails.
- **Slice D — per-workspace state.** Folder-keyed state map; URI routing (Q4);
  `did_change_workspace_folders`; the advertised capability made true. **The one
  structural dependency: lands after slice A** — it multiplies whatever the
  project model turns out to be.
- **Slice E — startup & watchers.** The documented startup analysis in
  `initialized`; dynamic `register_capability` for
  `workspace/didChangeWatchedFiles`, so a non-VS-Code client is notified;
  VS Code's client-side watchers kept working (no double-notification).
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

- **Slice A's cascade is wider than its diff looks.** Path identity touches
  every `Analysis` consumer and every fixture with a hardcoded `src`-relative
  path. Mitigation: the in-crate harness (§4.1), which needs no seam, plus
  `declaration_spans`/`hover_references`, which already read real
  `diagnose_project` output. Q5 decides whether the seam absorbs the fixture
  churn first.
- **Q3 has no free answer.** Refresh costs latency on the hot path; decline
  makes hover intermittently silent — arguably a worse experience than being
  occasionally wrong, and it will read as a regression. Measure before settling.
- **Slice D multiplies whatever slice A lands.** Ordering is load-bearing; a
  folder-keyed map over the wrong project model doubles the rework. This is the
  track's only forced ordering.
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
