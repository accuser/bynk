# 0071 — B-2 extension polish: scaffolds, commands, walkthrough, problem-matcher

- **Status:** Accepted (v0.38)
- **Relates to:** the v0.29 extension surface (the existing contributes)

## Context
The LSP server is feature-rich, but the VS Code extension contributed only the
language/grammar/semantic-tokens, four server-lifecycle commands, and three
settings — nothing that lowers first-use friction. B-2 adds authoring
affordances. The work is mostly extension-only; the one cross-cutting piece is a
machine-readable `bynkc` diagnostic format the problem-matcher keys off (the
default ariadne output is multi-line box-drawing and brittle to match).

## Decision
Ship B-2 across **two slices**:

- **Slice 1 (v0.38.0, extension-only):**
  - **Snippets** (`snippets/bynk.json`, `contributes.snippets`) — `context`,
    `commons`, `type`/`enum`, `fn`, `capability`, `provides`, `service`,
    `on http`/`on cron`, `agent`. Bodies mirror the worked fixtures so they
    type-check as written; tab-stops walk the names.
  - **Scaffolding commands** (`src/scaffold.ts`, registered eagerly in
    `activate()`): **`bynk.newContext`** writes a `context <name>.bynk` skeleton
    into `src/` (or the workspace root); **`bynk.newProject`** scaffolds
    `bynk.toml` + `src/<name>.bynk`. Both validate the name, **refuse to
    overwrite**, and open what they create — `workspace.fs` only, no new deps.
  - **Getting-started walkthrough** (`contributes.walkthroughs` + three markdown
    steps under `walkthroughs/`): welcome → create a project (a `New Project`
    command button, completed `onCommand:bynk.newProject`) → write a context.
- **Slice 2 (v0.38.1):** a terse **`bynkc check --format short`** renderer
  (`path:line:col: severity[category]: message`, severity from
  `Severity::for_error`, 1-indexed line/col) plus the `$bynkc`
  `contributes.problemMatchers` and a **`bynkc: check` build task** (a
  `TaskProvider` running `<bynkc> check . --format short` with `$bynkc`; the
  compiler resolves from a new `bynk.compilerPath` setting, else PATH). The LSP
  already reports diagnostics for *open* files; this catches the rest (unopened
  files, project-level errors) on demand. The one `bynkc` change, isolated.

## Consequences
Slice 1 is purely additive to `package.json` + two new files + two `activate()`
lines; it ships nothing into the LSP protocol and carries no compiler change.
The enforceable checks are the existing CI gate — `tsc --noEmit`, the esbuild
bundle, the bundle-require guard, and `vsce package` (which validates the
`contributes` schema and includes `snippets/`/`walkthroughs/` in the VSIX) —
plus a manual F5 smoke (no `@vscode/test-electron` harness exists; adding one is
a separate testing-infra increment). Marketplace/Open VSX publishing is CI
Tier 4 and out of scope.
