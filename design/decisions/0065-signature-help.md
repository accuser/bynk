# 0065 — Signature help: lexical call context, name-vs-value callees, shared renderer

- **Status:** Accepted (v0.32)
- **Spec:** `design/bynk-lsp-spec.md` §3.16
- **Relates to:** ADR 0061–0063 (completion), 0064 (locals); reuses their machinery

## Context
Comprehensive completion shipped over v0.30–v0.31. Its partner is **signature
help**: once the `(` is open, show the callee's signature with the active
parameter highlighted. After the completion work the pieces are nearly all in
place — the renderer, the registries, and the receiver-typing.

## Decision
Build `textDocument/signatureHelp` on the same shape as completion:

- **Context detection is lexical; signatures are semantic.** From the cursor,
  scan back to the **innermost unclosed `(`** (bracket-balanced — a depth-0 `[`
  or `{` means we're in type args / a list / a block, not a call), take the
  **callee** immediately before it, and the **active parameter** from a
  top-level (bracket-aware) comma count. Mid-edit safe, like the completion
  contexts.

- **Resolution mirrors completion's name-vs-value split.** **Name callees** —
  free functions, capability operations, refined/opaque `of`/`unsafe`, built-in
  type statics (`Int.parse`/`Json.decode`), and the `Ok`/`Err`/`Some`
  constructors — resolve from the recovery parse + the static registries, with
  **no typed model** (this increment). **Value-receiver** kernel methods
  (`xs.fold(`) need the receiver typed via the v0.30.2 machinery
  (`expr_types` + the rewrite) → a later slice, carrying the same clean-file
  ceiling.

- **One renderer, shared with hover.** Signatures render through
  `symbols::type_ref_str` and the fn/op signature helpers hover already uses
  (made `pub(crate)`), so the two surfaces never present two formats. The
  kernel/static **registry signature strings are reused verbatim** — they were
  authored as display strings for completion and are exactly what signature help
  shows. A small `param_ranges(label)` parses the parenthesised parameter list
  (top-level-comma-aware) into the LSP `ParameterInformation` offsets.

- **No overloads.** Bynk has one signature per callee, so the response is a
  single `SignatureInformation` with `activeParameter` set.

## Consequences
Signature help lands mostly as wiring — lexical detection + registry/parse
resolution + the shared renderer — reusing completion's `for_each_unit`,
`BUILTIN_STATICS`, `kernel_methods`, and (for the later value slice) the
receiver-typing. The accepted limits: value receivers wait for slice 2 (and
carry the clean-file ceiling); generic type-argument display in a signature
waits on the checker recording instantiations queryably (the queue's gate); and
`ParameterInformation` offsets are byte ranges (UTF-16-correct only for the
ASCII type signatures Bynk renders today — revisit if non-ASCII reaches a
signature).
