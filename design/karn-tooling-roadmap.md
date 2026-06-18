# Karn editor tooling — roadmap (LSP + VS Code)

A forward plan for the editor experience: the `karn-lsp` language server and the
`vscode-karn` extension that hosts it. Goal — a **complete, rich** editor experience
that rivals a modern language server, and a discipline that keeps it current as the
language grows. This is a design reference, not a per-increment proposal; concrete
slices become `proposals/` entries when scheduled.

> **Status note (refresh):** the A‑1/A‑2/A‑3 navigation and refactor features
> (references, rename, signatureHelp, codeAction, codeLens, inlayHint, semantic
> tokens, workspace symbols, call hierarchy, document highlights, implementation
> nav, folding/selection) **all shipped** across v0.24–v0.37; §1 below has been
> corrected to match, and §2's A‑lists now read as "done." What remains is the
> **completion debt** (still narrow) and the **B‑1/B‑2 editor polish**, which now
> live in their own connective plan: **[`tracks/lsp.md`](tracks/lsp.md)** — the
> completion gap analysis, the desirable-feature survey, and the slice
> decomposition. This roadmap stays the high-level parent.

---

## 0. Why the LSP feels absent today (fix first)

The extension spawns a separate **`karnc-lsp`** binary over stdio, discovered on `PATH`
or via `karn.executablePath` (`vscode-karn/src/extension.ts`); **the VSIX does not bundle
the server**. With no `karnc-lsp` on `PATH`, the editor still shows tmLanguage syntax
highlighting (no server needed) but **no hover, diagnostics, or completion** — the server
never starts. Start-up failures surface in the "Karn LSP" output channel and a toast, but
are easy to miss.

This is the single biggest gap: **a fresh install of the extension does not give a
working LSP.** It is roadmap item **B‑0** below, and it gates every other LSP feature
being *felt*.

---

## 1. LSP — current state

Implemented (`karn-lsp`, advertised in `main.rs`):

- **Live diagnostics** — recompiles via `karnc::diagnose` on change and publishes; these
  are the compiler's *authoritative* diagnostics, a genuine strength.
- **Hover** — signatures.
- **Go‑to‑definition.**
- **References, rename/prepareRename** (v0.25); **code actions** from diagnostics (v0.26);
  **signature help** (v0.32); **code lens** (v0.33); **inlay hints** (v0.27); **semantic
  tokens** full+range (v0.28); **workspace symbols** + **document highlights** (v0.26);
  **call hierarchy** (v0.34); **implementation nav** (v0.35); **folding + selection
  ranges** (v0.37) — i.e. the A‑1/A‑2/A‑3 table-stakes all shipped across v0.24–v0.37.
- **Completion** — *still narrow* and the main remaining debt: `consumes`/`given` plus
  positional/name-receiver/value-receiver contexts, but missing the `.` trigger char,
  expression-position breadth, free-function/stdlib completion, and builtin sum/static
  coverage. **The completion overhaul + the editor-experience remainder is planned in
  [`tracks/lsp.md`](tracks/lsp.md).**
- **Formatting** — document + range.
- **Document symbols**; **workspace folders.**

---

## 2. LSP — roadmap

### A‑0 — The foundation: a project‑wide semantic index ✅ *(shipped — the binding index, v0.25/ADR 0053)*

Most rich features (references, rename, workspace symbols, call hierarchy, document
highlights) need a **cross‑file symbol + reference graph**, not the current per‑document
recompile. Build (or expose, from `karnc`'s resolver/project analysis) a persistent
project model the server queries. **This gates all of A‑1's navigation/refactor work** —
do it first.

### A‑1 — Table‑stakes + the cheap Karn‑specific win ✅ *(shipped, except completion — code actions v0.26, references/rename v0.25, signature help v0.32)*

> Completion is the one A‑1 item still outstanding — it shipped partially and is the
> debt [`tracks/lsp.md`](tracks/lsp.md) closes.

- **Code actions from diagnostics** *(highest leverage — do early).* Karn's diagnostics
  are unusually **prescriptive** — they already say "add `X` to the `given` clause", "add
  a `consumes` for `B`", "construct via `T.of(...)`". Turning those notes into one‑click
  quick fixes is nearly free (the suggestion text exists) and makes Karn feel *more*
  polished than languages with vaguer diagnostics.
- **Find references** and **rename** (`prepareRename` + `rename`) — the two refactor
  table‑stakes; both ride A‑0.
- **Comprehensive completion** — today `consumes`/`given` only. Extend to: types, fns,
  **methods** (now incl. `List`/`Map`/`String` + generics), capabilities, record fields,
  enum variants, keywords, and snippets. (See §5 — this is partly catch‑up debt.)
- **Signature help** — parameter hints while calling fns/methods/capabilities (and lambda
  arguments to combinators).

### A‑2 — Rich experience ✅ *(shipped — inlay hints v0.27, semantic tokens v0.28, document highlights + workspace symbols v0.26, code lens v0.33)*

- **Inlay hints** — and these matter *more* for Karn now: v0.20a/b added inferred generic
  type args and lambda param types, and `let`‑binding types are inferred — all otherwise
  **invisible**. Hints for inferred `let` types, lambda params, and generic instantiations
  make that legible.
- **Semantic tokens** — type‑aware highlighting beyond tree‑sitter's syntactic pass:
  distinguish capability vs type vs **refined** vs **opaque** vs generic‑param vs
  **platform‑native** capability.
- **Document highlights** (occurrences of the symbol under cursor); **workspace symbols**
  (project‑wide search); **codeLens** (test‑run lenses, reference counts).

### A‑3 — Advanced *(partly shipped — call hierarchy v0.34, implementation nav v0.35; type-definition/type-hierarchy deferred at ADR 0068, now tracked in [`tracks/lsp.md`](tracks/lsp.md); file ops + on-type formatting + completion-resolve still open)*

- **Call hierarchy**; **type‑definition / implementation** navigation tuned to Karn —
  `given Cap` → its provider/adapter; a capability → its providers; a consumed context →
  its source.
- **File operations** — renaming a `.karn` file updates the unit name and consumers,
  given the source‑path‑mirrors‑qualified‑name rule.
- **On‑type formatting**; completion‑resolve (lazy docs).

---

## 3. VS Code extension — current state

`vscode-karn`: a tmLanguage grammar (syntax highlighting — works with no server), a
`language-configuration.json`, a `LanguageClient` that spawns `karnc-lsp` over stdio
(PATH or `karn.executablePath`), a status bar (project name from `karn.toml` + compiler
version), and an `openProjectConfig` command. Distributed as a VSIX (built at 0.17.0).
**The server is not bundled** — the extension assumes it is already on `PATH`.

---

## 4. VS Code extension — roadmap

### B‑0 — Server provisioning ✅ *(done — download‑on‑activate)*

A fresh install now provisions a working LSP. **Download‑on‑activate** was chosen over
per‑platform VSIX bundling: it ships on the existing release infrastructure (the raw
`karnc-lsp-<target>` binaries + `SHA256SUMS` the release now publishes) as one small VSIX,
and the `karn.executablePath` escape hatch covers offline/air‑gapped use. Implemented:

- **Resolution order** (`src/server.ts`): `karn.executablePath` → `karnc-lsp` on PATH →
  cached download (global storage) → download the pinned release binary, **verified against
  `SHA256SUMS`**, cached, `chmod 0o755`.
- **Loud, actionable failure** — an error toast with *Download Server / Open Settings / Show
  Output*, and a status‑bar item (`$(error) Karn LSP: not running`).
- **Commands** — `Karn: Restart Language Server`, `Karn: Download Language Server`,
  `Karn: Show Language Server Output`.
- **Version‑compatibility check** — warns when the running `karnc-lsp --version` differs from
  the extension's pinned `karnServerVersion` (package.json).
- **Release side** — `release.yml` publishes raw per‑target `karnc-lsp` binaries (+ checksums
  + provenance) so there is no in‑extension archive extraction.

*Note:* download needs a published Release at the pinned tag (`v0.23.0`); the infra is ready,
so cutting that release activates the path. Per‑platform VSIX **bundling** stays deferred to
Tier 4 (with marketplace publishing), if air‑gapped installs become a need.

### B‑1 — Surface the LSP's features in the UI

As A‑1/A‑2 land, wire the client so the features are *usable*: code‑action lightbulbs and
rename UI (mostly automatic once the server advertises them), an **inlay‑hint toggle** and
**semantic‑token theme** mappings, and codeLens for tests.

### B‑2 — Extension polish

- **Settings** — format‑on‑save, server trace level, inlay‑hint granularity.
- **Snippets** — `context`, `adapter`, `capability`, `service`, `on call`, `test` scaffolds.
- **Commands / scaffolding** — new project (`karn.toml` + layout), new context/adapter.
- **Tasks / problem matcher** — run `karnc` builds with diagnostics in the Problems panel.
- **Getting‑started walkthrough**; **marketplace publishing** (currently a hand‑built VSIX).

---

## 5. Cross‑cutting

- **Keep tooling current with the language — a standing rule.** The LSP is *accruing
  debt*: v0.20a/b and v0.21 added lambdas, generics, `List`/`Map`, JSON, soon `Float`, but
  completion is still `consumes`/`given`‑only and there's no inlay‑hint surface for the new
  inference. Each language increment's **tooling delta must explicitly enumerate LSP**
  (completion, hover, semantic tokens for the new constructs), not just tree‑sitter and
  fmt. Fold this into the proposal template's tooling‑delta line.
- **Editor‑agnostic.** The LSP is a standalone server; "rival modern languages" means a
  documented setup for **Neovim / Helix / Zed** too, plus a generic install path — not
  VS Code only.
- **Distribution / CI.** Build and bundle `karnc-lsp` per platform alongside the extension;
  publish to the VS Code Marketplace (and Open VSX for the non‑VS‑Code editors).

## 5.1 The `karn` driver & the project-lifecycle arc

Distinct from the LSP/extension thread above: a **`karn` driver** — a thin
orchestrator over `karnc` and the Node toolchain, as `cargo` is to `rustc` (ADR
0083). The compiler stays pure (compile / check / fmt / test); environment
orchestration lives in the driver. The arc is **`doctor` → `new` → `dev`**:

- **`doctor`** *(shipped v0.46)* — an upfront, capability-grouped environment
  check (compile / test / dev-deploy / editor / build-from-source), reporting
  presence + version + provenance, with a bare-informational / `--only` /
  `--strict` exit contract and `--format short|json` (ADRs 0083–0084). Chosen to
  go first because it has no language surface and mutates nothing, so it is the
  safe place to stand the driver up.
- **`new`** *(intent)* — scaffold a project (`karn.toml` + layout); overlaps
  B‑2's "Commands / scaffolding" line, which it supersedes for the CLI path.
- **`dev`** *(intent)* — build + watch + `wrangler dev` orchestration; carries
  the real design weight (incremental recompile, multi‑context worker selection,
  the v1 `workerd` dev‑server overlap noted in `karn-status-and-roadmap.md`).

`new`/`dev` are named as *intent*, not version‑pinned milestones.

## 6. Suggested sequencing

1. **B‑0** (server provisioning) — without it nothing else is *felt*. Smallest, highest impact.
2. **A‑1 code actions** (cheap, high polish) + **A‑0 index** in parallel (the index unblocks the rest of A‑1).
3. **A‑1** references / rename / completion / signature help.
4. **A‑2** inlay hints + semantic tokens (close the v0.20/v0.21 visibility debt) → the rest of A‑2.
5. **A‑3** + **B‑1/B‑2** polish; editor‑agnostic docs.

Each becomes a `proposals/` slice when scheduled; the LSP spec (`karn-lsp-spec.md`) is
updated in place as features land, the way the normative spec is.

---

## 7. Remaining backlog (the concrete queue)

_Subsumes the former `karn-tooling-proposal-queue.md` (status @ v0.43). The
v0.24–v0.43 line shipped nearly the whole original queue — comprehensive
completion, signature help, call hierarchy, implementation navigation,
folding/selection ranges, the inlay-hint follow-ups, the InRange quick-fix, B-2
extension polish. What follows is the short tail. Each line becomes a
`proposals/vX.Y-*.md` when scheduled; "gated" notes a prerequisite._

**Advertised today:** hover, definition, completion (types, fns, members,
locals, keywords, snippets), formatting (+range), document symbols, references,
rename, code actions, inlay hints (types, parameter names, generic
instantiation), semantic tokens, workspace symbols, document highlights,
signature help, CodeLens (reference counts), call hierarchy, implementation
navigation, folding & selection ranges.

### 7.1 Open tooling work (server, `karnc` + `karn-lsp`)

1. **Locals-rename + generic type parameters** — the last unpaid slice of the
   recurring index deferral (v0.25/v0.27/v0.28/v0.31/v0.36). Local bindings
   resolve and colour, but **rename** for them is still deferred (subtler
   scope/shadowing edits); **generic type parameters** are not indexed at all.
   Also out: match-arm / `is`-narrowing pattern bindings, and the
   `parameter`-vs-`variable` token split. *Meaty; sliceable. Highest-value
   remaining item.*
2. **Type-definition navigation** — `textDocument/typeDefinition`: value → its
   type, consumed context → its source. (The sibling, implementation
   navigation, shipped v0.35.) *Medium.*
3. **Test-run CodeLens** — the "▶ Run" lens above tests. *Gated:* needs test
   discovery + a run command. *Small once the gate lands.*
4. **`inlayHint/resolve`** — lazy hint tooltips. *Small, ungated.*

### 7.2 Deferred optimisations (do when scale demands)

5. **Semantic-tokens `delta`** — re-encode only changes. *No scale signal yet.*
6. **Incremental recompute** — a salsa-style incremental recompute replacing the
   per-debounce full project analysis. *Deferred since v0.24.*

### 7.3 Distribution

Extension + grammar release automation (Marketplace + Open VSX), per-platform
VSIX bundling, and binary signing/notarisation are **CI Tier 4** and live in
[`karn-engineering-roadmap.md`](karn-engineering-roadmap.md) — both gated on
marketplace tokens / signing certificates.

### 7.4 Not tooling, but it gates tooling

- **`given` on free functions** (the v0.23 discovered limitation) — language
  core; until it lands, no capability can be driven from a factored helper,
  which caps what capability-iteration tooling can demonstrate.
