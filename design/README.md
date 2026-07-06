# Design notes

Internal design records for Bynk. These are the working references behind the
language and tooling — distinct from the published book under `docs/`, which is
the canonical, reader-facing spec and reference.

## Current state

**Status & roadmaps** — where the project is and where it's going:

- [`bynk-status-and-roadmap.md`](bynk-status-and-roadmap.md) — the **status & gap
  audit** (refreshed per release; currently v0.54). Start here. The root
  `README.md` links to it.
- [`bynk-tooling-roadmap.md`](bynk-tooling-roadmap.md) — the editor-experience
  forward plan (LSP + VS Code), including the remaining tooling backlog.
- [`bynk-engineering-roadmap.md`](bynk-engineering-roadmap.md) — the CI/CD
  pipeline plan and the `bynkc` internal-quality refactor backlog.

**Canonical design** — the long-form rationale and the type theory:

- [`bynk-design-notes.md`](bynk-design-notes.md) — the long-form design rationale
  (the aspirational v1 language).
- [`bynk-type-system.md`](bynk-type-system.md) — the type system in depth
  (aspirational; carries an implementation-status banner).

**Tooling specs** — capability contracts, referenced from code:

- [`bynk-lsp-spec.md`](bynk-lsp-spec.md) — LSP capabilities; referenced from
  `bynk-lsp/src/main.rs`, `bynkc/src/fmt.rs`, and ~18 ADRs.
- [`bynk-tree-sitter-spec.md`](bynk-tree-sitter-spec.md) — tree-sitter highlight
  groups; referenced from `tree-sitter-bynk/queries/highlights.scm`.

**Process directories:**

- [`proposals/`](proposals/README.md) — **increment proposals**: the transient
  sign-off artefact for an increment, living as a **GitHub issue** (label
  `proposal`; `accepted` = approval to build) and closed by the PR that
  implements it. The directory holds only long-form untracked drafts.
- [`tracks/`](tracks/README.md) — **feature-track design docs** (ADR 0076, run
  GitHub-native per ADR 0167): the *persistent* design + slice decomposition for
  a far-reaching, multi-increment language feature. A `track` issue is the
  track's spine, a **draft PR** settles the doc, and slice proposals are opened
  as sub-issues of the spine. Unlike a proposal, a track doc is not consumed on
  merge; it is the living map the per-slice proposals are cut from.
- [`decisions/`](decisions/README.md) — the **decision records**: one ADR per
  language-defining call, harvested from the retired increment instalments and
  added per increment going forward.
- [`archive/`](archive/README.md) — superseded and shelved docs, kept for the
  record. Nothing there is current.

**Other:**

- [`bynk-phd-exploratory-memo.md`](bynk-phd-exploratory-memo.md) — exploratory
  research memo (not a language design doc).

## Versioning & release

The repo carries a **single version** while everything lives together. The
sites that must agree — the Cargo workspace (`[workspace.package]` plus the
in-workspace dependency requirements), `vscode-bynk` (`version` *and*
`bynkServerVersion`, the GitHub Release the extension downloads server
binaries from), and `tree-sitter-bynk` — are all set by one command:

```sh
scripts/bump-version.sh X.Y.Z
```

The extension pin is why drift is behavioural, not cosmetic: a trailing
`bynkServerVersion` means users get a stale compiler even after a release.

Per release:

1. The **implementing PR** runs the bump script and lands the version bump
   with the increment (alongside the spec/changelog deltas).
2. To ship a version, **tag `vX.Y.Z`** — the release workflow then does the
   whole release from that one tag: `verify` (tests + tag/version match) →
   build the binaries + cut the GitHub Release, **and** publish the crates to
   crates.io and the grammar to npm (both via OIDC Trusted Publishing, both
   re-run-safe — a version already on a registry is skipped, so a partial
   publish can be retried by re-running the run).
3. A release tag is cut when a version is to be shipped (not necessarily every
   increment) — the GitHub Release the extension's `bynkServerVersion` pin
   points at must exist. A manual `workflow_dispatch` against the tag re-runs
   just the registry publishes (the override / retry path).

The release workflow's `verify` job refuses a tag whose version does not
match **all** of the sites above. The registry publishes are irreversible; the
`verify` gate stands in for an approval gate (an Environment with required
reviewers can add a one-click pause once the repo is public).

## History

The per-increment grammar instalments (`grammar-increments/`,
`bynk-adapters-spec.md`) have been **removed**: the normative spec in
`site/src/content/docs/book/spec/` is the single source of truth for the shipped language, updated
in place per increment. The instalments' history lives in version control; the
design decisions they recorded live on in [`decisions/`](decisions/README.md).
