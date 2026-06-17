# Documentation coverage audit — language features

**Date:** 2026-06-17 · **Against:** `docs/src/**` (the Karn Book), the diagnostics
registry, and `tree-sitter-karn/src/grammar.json`.

> **Resolved (2026-06-17).** All three gaps below are addressed:
> 1. **`Float`** — added to `reference/types.md` (base-types table + the
>    `Int`/`Float` incompatibility and conversions) and to the refinement guide
>    `guides/type-system/define-and-validate.md`.
> 2. **`Json` / `JsonError`** — a curated "JSON codec" entry in
>    `reference/types.md` plus a new how-to,
>    `guides/type-system/decode-json.md` (decode untrusted JSON into a typed/
>    refined value), wired into the TOC.
> 3. **Stale track README** — `design/tracks/README.md` now reads
>    "✅ COMPLETE — Q1–Q7 shipped".
>
> Kept as a historical snapshot of the sweep.

## What was checked

Every language feature was swept along two axes and checked for presence and
completeness across **four doc layers**: reader-facing (tutorials + guides +
introduction), the **Reference** catalogue, the **Specification**, and
**diagnostics**.

The feature set was drawn from both authoritative enumerations:

- **57 reserved keywords / constructs** (`docs/src/reference/keywords.md`,
  generated from the lexer).
- **101 named grammar productions** (plus hidden rules) in
  `tree-sitter-karn/src/grammar.json` — 118 embeddable rules in total.

## Headline

The docs are in **excellent shape**. Three of the four layers are largely held
true by CI, not goodwill, and the substantive gaps come down to **two base-level
types — `Float` and `Json`/`JsonError` — that have no reader-facing coverage and
are missing from the curated type reference.** Everything else is covered across
all layers.

## What CI already guarantees (so it cannot drift)

Four guard tests in `karnc/tests/` enforce coverage mechanically:

| Guarantee | Test | Effect |
|---|---|---|
| Reference covers **every** grammar production (1:1 bijection + anchors) | `grammar_coverage.rs` | A new production cannot ship without a documented `{{#grammar}}` entry and a resolving `#rule-<x>` anchor. |
| Generated reference pages match source | `grammar_reference.rs`, `keywords_reference.rs`, `cli_reference.rs`, `diagnostics_registry.rs` | `grammar.md`, `keywords.md`, `cli.md`, `diagnostics.md` cannot drift from the compiler/grammar. |
| Every diagnostic is catalogued + mapped to a construct | `diagnostics_registry.rs` | All 212 diagnostic codes appear in `reference/diagnostics.md`; `grammar-semantics.json` links 63 constructs to their rules. |
| Every fenced `karn` example compiles; refusals are real | `doc_examples.rs`, `doc_diagnostics.rs` | No stale or non-compiling snippet survives CI. |
| Book version banners match the release | `doc_version.rs` | Single-sourced; no manual bump drift. |

Consequently the **Reference** layer is effectively complete for all 57 keywords
and all 118 grammar productions, and the **diagnostics** layer is complete by
construction. Changelog and spec version history are aligned at **v0.54**.

## Coverage matrix (reader / reference / spec)

Whole-word presence across the curated prose layers. Of the 57 keywords, **55**
appear in all three prose layers. Exceptions:

| Feature | Reader | Reference (curated) | Spec | Verdict |
|---|---|---|---|---|
| `Float` | ✗ absent | ✗ absent from `types.md` (only generated pages) | ✓ `type-system.md` §6 | **Gap** |
| `Json` / `JsonError` | ✗ absent | ✗ no curated entry (only `adapters.md` mention) | ✓ type-system / emission / static-semantics | **Gap** |
| `protocol` | ✗ (keyword) | ✓ | ✓ | OK — reserved; the protocols themselves (`http`/`cron`/`queue`) are fully covered |
| `expect` | n/a | ✓ (keyword table) | — | OK — reserved placeholder, no semantics yet |
| `record` | ✓ | ✓ | ✓ | OK — reserved spelling; records written as `type X = { … }`, fully covered |

The newest feature, **actors** (v0.54), is complete across all four layers —
tutorials, guides (`guides/actors/`), reference (`reference/actors.md`),
spec (`static-semantics.md`, `emission.md`, `runtime-library.md`,
`syntactic-grammar.md`), and diagnostics.

## Gaps to act on

### 1. `Float` — no prose anywhere outside the spec

`Float` has been a primitive type since v0.21. It is enumerated in the generated
pages (grammar, diagnostics, keywords) and described in `spec/type-system.md`,
but it appears in **no tutorial or guide** and is **not mentioned in the curated
`reference/types.md`**. A reader learning the type system never meets it.

*Fix:* add `Float` to the primitives in `reference/types.md` (alongside `Int`,
`String`, `Bool`), and a sentence or short example in
`guides/type-system/define-types.md` — ideally noting the `Int`/`Float`
incompatibility the spec already calls out.

### 2. `Json` / `JsonError` — decoding has no reader or curated-reference home

`Json.decode` returns `Result[T, JsonError]` and is a real, shipped capability,
but `Json`/`JsonError` appear only in `adapters.md` and the generated pages on
the reference side, and **nowhere** in tutorials or guides. There is no page that
teaches decoding untrusted JSON into a typed/refined value — arguably a flagship
use case given the "make illegal states unrepresentable" pitch.

*Fix:* add a curated reference entry (in `reference/types.md` or
`reference/karn-capabilities.md`) and a short how-to under
`guides/type-system/` (it pairs naturally with *Define and validate untrusted
input*).

### 3. Minor — stale internal track doc (not part of the Book)

`design/tracks/README.md` still lists actors as **"Phase: design exploration
(pre-slice)"**, whereas `design/tracks/actors.md` correctly reads **"✅ COMPLETE
— Q1–Q7 shipped"** and the changelog shows v0.45–v0.54 actor slices landed. This
is internal design documentation, not the user-facing book, but the README line
should be updated (or the track retired per its own lifecycle step 3).

## Notes / non-gaps

- **Troubleshooting** (12 pages) is intentionally curated for common pitfalls,
  not 1:1 with the 212 diagnostics — `reference/diagnostics.md` is the complete
  index. No action needed.
- Reserved-but-inactive keywords (`expect`, `protocol`) are minimal by design and
  correctly documented as reserved.
- Grammar productions with no `grammar-semantics` mapping are unconstrained
  productions; the preprocessor emits a neutral line by design, not a gap.
