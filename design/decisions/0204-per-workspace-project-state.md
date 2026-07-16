# 0204 — Per-workspace project state: route by discovered project root, not workspace folder

- **Status:** Accepted (v0.182)
- **Provenance:** proposed in #673, a slice of the LSP foundations work (spine
  #640). The routing model it fixes — how the protocol's multi-root capability
  maps onto Bynk's projects — was settled as **Q4** (#672) before the slice was
  cut; this record fixes the mechanism and is its durable home once the track
  doc retires.
- **Relates:** [[0201]] (the project model this multiplies — one manifest-aware
  discovery, now held once per root), [[0202]] (the freshness gate, now per
  project), [[0052]] (project diagnostics — the round each entry runs), [[0147]]
  (the flat `include`/`exclude` layout `resolve_root` obeys), [[0156]] (the
  editor surface is a projection of the language — a request answered against the
  wrong project betrays it), [[0023]] (each increment stays single-purpose).

## Context

The server advertised the workspace-folders capability
(`workspace_folders: { supported: true, change_notifications: true }`) and did
not implement it. State was one project: a single `project_root: Option<PathBuf>`
populated from `folders.first()` at `initialize`, one `config`, one `analysis`,
one published set, one generation counter. Additional workspace folders were
silently dropped, and there was no `workspace/didChangeWorkspaceFolders` handler
anywhere in the crate. A file in a second folder — or a second `bynk.toml`
project *under one* folder, a monorepo's shape — was analysed against the first
project's model, or against none. This was the third of the four foundational
gaps [[0201]] and [[0202]] closed the first two of: the lifecycle advertised more
than it implemented.

The load-bearing question was not *whether* to implement it (the track scoped
that: implement, do not withdraw) but *how a URI maps to a project* once there is
more than one — which folder owns a file when folders nest, what answers a file
under no folder, and whether file-watching registers per folder or once. Those
are Q4.

## Decision

**(A) Route by the discovered project root, not the workspace folder.** State is
a map `projects: HashMap<PathBuf, ProjectState>` keyed by **canonical project
root**. A request routes by `Backend::resolve_root(uri)` — the existing walk
upward to the file's nearest enclosing `bynk.toml` (else nearest `src/`, else
`None`) — which is the *same* project `bynkc` attributes the file to. So the
server keeps agreeing with the compiler about which project a file is in, by
reusing one walk-up rule rather than inventing a second, folder-shaped one.

Nesting then has no tie to break: overlapping or nested folders resolve to
whatever set of `bynk.toml` roots lies beneath them, and each file lands in its
nearest. Two folders sharing one root share one entry; one folder holding two
roots yields two. **Workspace folders are discovery seeds** — they bound where
the server discovers and prunes projects — **never the routing key.**

**(B) A file under no project stays in single-file mode.** When `resolve_root`
returns `None`, the file is diagnosed per-buffer (`bynk_ide::diagnose`), exactly
as before; index-backed handlers decline for it (they return nothing, they do
not error). This is not a new decision — it is what the server already did — and
Q4 fixed it as the answer: decline is for the genuinely unanswerable (single-file
mode, a file outside every `include` root, [[0202]]'s raced refresh), never for
"outside every folder."

**(C) `workspace/didChangeWorkspaceFolders` maintains the seed set and prunes.**
Added folders extend it; projects under them resolve lazily on first touch (the
path `did_open` already takes). Removed folders shrink it, and any project no
longer reachable from a remaining folder — *and* holding no open buffer — is
dropped and its diagnostics cleared. A project with an open buffer is retained
until its last buffer closes, because routing still needs it — so the **same
prune runs from `did_close`**: a project falls only when *both* its seed (folder)
and its buffers are gone, whichever leaves last. One `prune_orphaned_projects`
helper serves both events, so the two paths cannot drift. Proactive analysis
of a folder *before* any file opens is **not** here: `initialized` runs no
startup round today, so a lazy map preserves current behaviour exactly, and the
proactive startup scan is a later slice (E) that reuses one tree-walk for both
startup and added folders.

**(D) One global `workspace/didChangeWatchedFiles` registration.** The globs
Bynk watches — `**/*.bynk`, `**/bynk.toml` — are folder-independent, so a single
registration covers every folder and a file event's absolute URI routes through
`resolve_root` regardless of which registration matched. `didChangeWorkspaceFolders`
therefore never re-registers watchers. (Dynamic registration itself is slice E;
Q4 settles that it is one global registration, not one per folder.)

**(E) `workspace/symbol` is the one cross-project query.** It aggregates over
every open project — candidate roots being those already touched plus each
folder's own root, so a query answers before any file is opened, as the
single-project server did. Every other handler answers from one project, routed
by its URI.

*The subtlety recorded, because it is the load-bearing detail:* the map key and
every `Analysis.project_root` are the **canonical** root, and routing
canonicalises too — so on a platform where the workspace path is a symlink
(macOS `/var` → `/private/var`), a request's URI-derived root still lands on the
same entry the round created. Keying by the raw, uncanonicalised root would make
a request route to a different key than the round that filled it, and every
index-backed feature would silently find nothing.

## Consequences

- **The rename gate is per project.** [[0202]]'s `analysis_covering_open_buffers`
  takes the rename's root; a rename spans one project (the symbol and its
  references live under one root), so a stale buffer in *another* project cannot
  refuse it. A buffer outside the rename's project strips against a different
  `project_root`, so it does not gate.
- **Overlay isolation is free, not filtered.** A project's round overlays all
  open buffers, but a buffer belonging to another project keys to an absolute
  path outside this root — discovery never matches it, and the `strip_prefix`
  guard skips its version entry — so the round stays scoped without filtering the
  doc set. The published set and freshness generation are per entry, so one
  project's round never clears or supersedes another's.
- **The advertised capability is now true**, closing the review's finding 3. The
  server implements exactly the multi-root surface it advertises.
- **Tooling ([[0156]]).** Hover, completion, semantic tokens, and signature help
  are **unchanged** — this slice changes *which project* answers a request
  (routing), not any handler's logic. Stated explicitly, not inherited.
- **Tests are behaviour-over-time** (drive a real `Backend`): two projects under
  one folder analyse independently and in isolation; folder removal prunes an
  idle project and retains one with an open buffer; a rename in one project
  ignores a dirty buffer in another. They sit beside the single-project tests
  [[0201]]/[[0202]] wrote, which pass unchanged — the proof the routing is
  additive.
- **What this does not do.** Proactive startup analysis and dynamic watcher
  *registration* are slice E; a project a folder holds but no file has touched is
  discovered on first open, not scanned ahead. No incremental layer is added — a
  refresh is still a whole-project round ([[0202]]'s named scaling cliff),
  per project.
