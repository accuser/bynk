# Design notes

Internal design records for Karn. These are the working references behind the
language and tooling — distinct from the published book under `docs/`, which is
the canonical, reader-facing spec and reference.

## Current state

- [`proposals/`](proposals/README.md) — **active increment proposals**: the
  transient sign-off artefact for an increment, deleted by the PR that
  implements it.
- [`decisions/`](decisions/README.md) — the **decision records**: one ADR per
  language-defining call, harvested from the retired increment instalments and
  added per increment going forward.
- [`karn-status-and-roadmap.md`](karn-status-and-roadmap.md) — where the project
  is and where it's going.
- [`karn-design-notes.md`](karn-design-notes.md) — the long-form design rationale.
- [`karn-type-system.md`](karn-type-system.md) — the type system in depth.
- [`karn-lsp-spec.md`](karn-lsp-spec.md) — LSP capabilities; referenced from
  `karn-lsp/src/main.rs` and `karnc/src/fmt.rs`.
- [`karn-tree-sitter-spec.md`](karn-tree-sitter-spec.md) — tree-sitter highlight
  groups; referenced from `tree-sitter-karn/queries/highlights.scm`.
- [`karn-phd-exploratory-memo.md`](karn-phd-exploratory-memo.md) — exploratory
  research memo.

## History

The per-increment grammar instalments (`grammar-increments/`,
`karn-adapters-spec.md`) have been **removed**: the normative spec in
`docs/src/spec/` is the single source of truth for the shipped language, updated
in place per increment. The instalments' history lives in version control; the
design decisions they recorded live on in [`decisions/`](decisions/README.md).
