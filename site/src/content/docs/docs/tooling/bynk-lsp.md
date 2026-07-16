---
title: "`bynk-lsp`"
---
The Bynk language server. The `bynk-lsp` crate builds the `bynkc-lsp` binary, a
[tower-lsp](https://github.com/ebkalderon/tower-lsp) server that communicates
over **stdio**. Editors talk to it; most users reach it through the
[VS Code extension](/docs/tooling/vscode-bynk/) rather than directly. See the
[Set up editor support](/docs/editor-and-tooling/editor-support/) how-to for
wiring it into an editor.

Text is synchronised in full: on every change the whole document is re-sent
(`TextDocumentSyncKind::FULL`), and the server holds the current buffer in
memory. When a project root with a `bynk.toml` is found, the server enables
cross-file features; otherwise it operates in single-file mode. Project
discovery and the analysis model that underpins these features are described in
[Architecture](#architecture) and [Project discovery and performance](#project-discovery-and-performance).

## Capabilities

Every capability below is advertised in the server's `ServerCapabilities` and
backed by a request handler.

| Capability | What it does |
|---|---|
| Diagnostics | Recovering compilation surfaced as squiggles — live by default (debounced, ~200–300 ms) and re-run when watched files change. With a project root diagnostics are project-wide: the whole bundle is analysed, open buffers overlaid on disk, so an error in one file shows on the file that owns it. |
| Hover | Type signatures and doc blocks for the symbol under the cursor, resolved through the binding index so the description matches the actual definition — a name-match fallback answers only where the index does not resolve. Works on a name's *uses*, not just its declaration: inside an agent handler body, a `store`/`key` field reference describes the field, a record-construction label (`Stored { title: … }`) describes the type's field, and a store operation (`items.put(…)`) describes the operation over the field's declared kind. Every kind the index carries is described: an actor (`by u: User`), a method — the one the call *binds* to, so `g.bump()` describes `Gauge.bump` and not a same-named `Counter.bump` — and a capability operation, attributed to the capability that declares it, including where a project declares one whose name a built-in shares. |
| Go-to-definition | Jumps to the declaration of types, functions, capabilities, services, agents, providers, and actors (a handler's `by u: User` clause) — cross-file, via the project index. Local bindings resolve scope-correctly; `uses`/`consumes` unit segments jump to the unit's source. |
| Go-to-type-definition | From a value to the declaration of its inferred type. Reads the value's type from the round's expression types and lands on the named type's declaration. |
| Find references | Project-wide occurrences from the binding index, including clause lists and test units. Local bindings return their definition plus uses within the file. |
| Rename | Project-wide rename; `prepareRename` validates that the symbol is in scope and refuses out-of-scope kinds. Emits versioned edits, and re-analyses with the edits applied to reject collisions and silent re-bindings before returning. |
| Formatting | Whole-document formatting via the shared [`bynk-fmt`](/docs/tooling/bynk-fmt/); a parse error yields no edits (the diagnostic flow reports it). |
| Range formatting | Partial-document formatting. Per spec it may return edits wider than the requested range. |
| Document symbols | The file outline for the editor's symbol view and quick-open. |
| Completion | Scope- and context-aware: units after `consumes `, capabilities inside `consumes U { … }` and after `given `, in-scope locals at keyword and expression positions, and members after `.` on a typed value receiver. Documentation is resolved lazily on the focused item so the initial list stays cheap. |
| Signature help | The active parameter of the call being typed, triggered on `(` and `,`. Covers named callees and value-receiver methods. |
| Code lens | A reference-count lens above each top-level definition, clickable to peek the references. |
| Call hierarchy | Incoming and outgoing calls over the binding index's call graph. |
| Implementation | From a capability to its providers (the reverse direction, provider to capability, is served by go-to-definition). |
| Document links | `uses`/`consumes` unit names — and a test file's `suite <target>` header — become clickable links to the unit's source file. |
| Document highlight | The matching binding's occurrences highlighted across the active file. |
| Folding ranges | Structural folds and comment runs, driven by the recovered AST (no analysis round needed). |
| Selection ranges | Expand-selection by syntactic nesting — the enclosing-node chain for each position. |
| Code actions | Quick-fixes built from the structured suggestions carried on diagnostics, served from the cached round so they agree with the squiggles on screen. |
| Inlay hints | Inferred-type hints for the visible range, plus materialisable ghost `given` hints for uncovered capability requirements. |
| Semantic tokens | Resolution-aware highlighting (full document and range), additive over the client's syntactic layer, read from the cached index. |
| Workspace symbols | Symbol search across the index's definitions, filtered by query — aggregated over **every** open project (the one cross-project query in a multi-root window). |
| File watching | Re-checks diagnostics when `.bynk` files change on disk, and reloads a project's `bynk.toml` when the manifest changes. The server **registers the watchers itself** (dynamic `didChangeWatchedFiles`), so any client is notified — not just those that supply watchers client-side. |
| Startup analysis | On activation the server discovers and analyses every project under the workspace folders, so diagnostics appear **before any file is opened**. Folders added later are warmed the same way. |
| Workspace folders | Real multi-root support: several projects open at once, each analysed independently; a file routes to its nearest `bynk.toml`. Adding or removing a folder adds or prunes its projects. |

## Build

From the workspace root:

```sh
cargo build --release -p bynk-lsp
```

The binary is `target/release/bynkc-lsp`. Put it on `PATH`, or point your editor
at it explicitly (in VS Code, the `bynk.executablePath` setting).

`bynkc-lsp --version` prints the version and exits without entering the protocol
loop, so tooling (such as the VS Code status bar) can query it without the
server blocking on stdin.

## Architecture

The `Backend` holds the mutable server state behind a `tokio::sync::RwLock`:

- a **map of projects** keyed by discovered root — one editor window may hold
  several. Each entry carries its own parsed **configuration**, analysis round,
  freshness generation, and published-diagnostics set (so two projects analyse
  and publish independently),
- the **workspace-folder seeds** — where projects are discovered and pruned, not
  how requests route, and
- the **open documents**, keyed by URI, each with its current text and version —
  a client-global set; each doc routes to its project by its nearest `bynk.toml`.

A document change runs `recompile_and_publish`, which routes the URI to its
project. In single-file mode (no manifest) that diagnoses the one buffer
directly; in a project it schedules that project's debounced project-wide round.
Hover and go-to-definition first consult the binding index,
falling back to a re-parse of the AST under the cursor for kinds the index does
not carry; formatting delegates to `bynk-fmt`.

### The analysis round

Each project-wide analysis retains one round's outputs, held together so that
every position converts against the text the analysis actually saw — not the
live buffer, which may already have moved on. A round carries:

- the **binding index** — the call graph and cross-file symbol table that
  references, rename, definition, hover, call hierarchy, implementation, and
  workspace symbols all read;
- per-file analysed **snapshots** — the exact text each span is an offset into;
  every span-to-position conversion uses these;
- the **open-document versions** captured when the overlay was built — the
  freshness contract gates on these (below), rename emits versioned edits
  against them, and published diagnostics carry them so a client can drop a
  range its buffer has moved past;
- the full **diagnostics** per file, including the structured suggestions that
  code actions ride on (clean files retain an empty entry);
- **inferred-type hints** and the **capability-requirement ledger** that drive
  the two kinds of inlay hint;
- **local bindings with scope ranges**, for scope-correct local navigation;
- **expression types**, which back go-to-type-definition; and
- a **unit-name-to-source map**, which backs document links.

**The freshness contract.** An index-backed request always answers against the
buffer the client currently holds. Each analysed file records the document
version it was analysed at; when a request arrives for a file whose buffer has
moved past the last round, the server refreshes — runs a round over the current
buffers — before answering, so a position is never resolved against text the
user has already edited past. When it cannot reach the requested version (a file
outside the project, or a concurrent edit that raced the refresh) it returns
nothing rather than a stale answer. A round is fast (single-digit milliseconds
for a typical project), and concurrent requests after one edit share a single
refresh.

## Project discovery and performance

The server resolves each file to its project by walking upward from the file to
the nearest `bynk.toml` (the same attribution `bynkc` gives it) — so several
projects across one window's workspace folders each analyse independently, and a
file's workspace folder does not decide its project. Whether a file resolves to
a project decides its feature set:

- **In a project** — cross-file lookups, project-wide diagnostics,
  workspace symbols, rename, and the index-backed navigation features all apply.
  The files analysed are **exactly the files `bynkc` compiles**: every tree in
  the manifest's [`[paths] include`](/docs/manifest/), with `exclude` honoured,
  read through the compiler's own discovery. A conventional project needs no
  `[paths]` at all — `src/` and, when present, `tests/` are picked up; a flat
  project (`.bynk` at the root) works as-is.
- **Single-file mode (no manifest)** — each buffer is analysed on its own and
  the workspace features are unavailable; diagnostics still work per buffer.

Diagnostics are debounced (~200–300 ms), configurable via the
[`[lsp]` key](/docs/manifest/) `diagnostics_debounce_ms` in `bynk.toml`. A
generation counter guards the debounce: every change bumps it, and a scheduled
round runs only if it is still the latest when the delay elapses, so a burst of
keystrokes coalesces into a single analysis. The analysis itself runs off the
async runtime. There is a real, if narrow, window between analysing a round and
publishing it — which is exactly why positions convert against the analysed
snapshots rather than the live buffer.

## Internals

The crate is split into focused modules:

| Module | Role |
|---|---|
| `main.rs` | Server entry point, `Backend` state, request dispatch, advertised capabilities. |
| `position.rs` | Byte-offset ↔ LSP position conversion. |
| `symbols.rs` | Symbol lookups for hover and go-to-definition. |
| `hover.rs` | Hover's resolution ladder — the order the lookups are tried in, as one pure function the handler and its tests share. |
| `index_queries.rs` | Pure queries over the project binding index: references, rename planning and validation, call hierarchy, semantic tokens, code lenses. |
| `completion.rs` | Context detection and candidate generation for completion. |
| `signature_help.rs` | Call-context detection and signature labels. |
| `inlay_hints.rs` | Inferred-type and ghost `given` hint rendering. |
| `code_actions.rs` | Quick-fixes from diagnostics' structured suggestions. |
| `locals_nav.rs` | Scope-correct navigation for local bindings. |
| `structure.rs` | Folding and selection ranges from the recovered AST. |
| `document_symbols.rs` | The document-symbol outline. |
| `publish.rs` | The pure publish plan (which files to publish, which to clear). |
| `project.rs` | `bynk.toml` project configuration. |

## Logging

The server logs to `~/.bynk-lsp.log`; the verbosity is tunable via the
`BYNK_LSP_LOG` environment variable (default `warn`).
