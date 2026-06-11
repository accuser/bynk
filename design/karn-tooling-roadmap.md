# Karn editor tooling — roadmap (LSP + VS Code)

A forward plan for the editor experience: the `karn-lsp` language server and the
`vscode-karn` extension that hosts it. Goal — a **complete, rich** editor experience
that rivals a modern language server, and a discipline that keeps it current as the
language grows. This is a design reference, not a per-increment proposal; concrete
slices become `proposals/` entries when scheduled.

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
- **Completion** — *narrow*: only `consumes` units and `given` / `consumes U { … }`
  capabilities.
- **Formatting** — document + range.
- **Document symbols**; **workspace folders.**

Not implemented (the spec's §4.3 list): references, rename/prepareRename, signatureHelp,
codeAction, codeLens, inlayHint, semantic tokens, workspace symbols, call hierarchy,
document highlights.

---

## 2. LSP — roadmap

### A‑0 — The foundation: a project‑wide semantic index

Most rich features (references, rename, workspace symbols, call hierarchy, document
highlights) need a **cross‑file symbol + reference graph**, not the current per‑document
recompile. Build (or expose, from `karnc`'s resolver/project analysis) a persistent
project model the server queries. **This gates all of A‑1's navigation/refactor work** —
do it first.

### A‑1 — Table‑stakes + the cheap Karn‑specific win

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

### A‑2 — Rich experience

- **Inlay hints** — and these matter *more* for Karn now: v0.20a/b added inferred generic
  type args and lambda param types, and `let`‑binding types are inferred — all otherwise
  **invisible**. Hints for inferred `let` types, lambda params, and generic instantiations
  make that legible.
- **Semantic tokens** — type‑aware highlighting beyond tree‑sitter's syntactic pass:
  distinguish capability vs type vs **refined** vs **opaque** vs generic‑param vs
  **platform‑native** capability.
- **Document highlights** (occurrences of the symbol under cursor); **workspace symbols**
  (project‑wide search); **codeLens** (test‑run lenses, reference counts).

### A‑3 — Advanced

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

## 6. Suggested sequencing

1. **B‑0** (server provisioning) — without it nothing else is *felt*. Smallest, highest impact.
2. **A‑1 code actions** (cheap, high polish) + **A‑0 index** in parallel (the index unblocks the rest of A‑1).
3. **A‑1** references / rename / completion / signature help.
4. **A‑2** inlay hints + semantic tokens (close the v0.20/v0.21 visibility debt) → the rest of A‑2.
5. **A‑3** + **B‑1/B‑2** polish; editor‑agnostic docs.

Each becomes a `proposals/` slice when scheduled; the LSP spec (`karn-lsp-spec.md`) is
updated in place as features land, the way the normative spec is.
