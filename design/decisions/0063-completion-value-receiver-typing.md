# 0063 — Value-receiver `.`-member completion: rewrite, retained `expr_types`, enumerable kernels

- **Status:** Accepted (v0.30.2)
- **Spec:** `design/karn-lsp-spec.md` §3.15
- **Refines:** ADR 0062 (the `.`-member receiver split)

## Context
ADR 0062 shipped name-receiver `.`-members and deferred the **value**-receiver
half (`list.fold`, `str.split`, `order.total`) because it needs the receiver's
*type* — and a spike was needed to know if that's even obtainable on a mid-edit
buffer. The spike answered yes, with a ceiling:

- **The mid-edit parse loses the receiver.** A bare trailing `receiver.` (the
  auto-trigger instant) cascades — the parser consumes the closing `}`, the
  receiver vanishes, no `FieldAccess`. But `receiver` (dot dropped) types
  cleanly.
- **`expr_types` is computed but discarded.** The checker builds
  `expr_types: Span → Ty` per file, but it rides in the `Ok(TypedCommons)`
  payload `check_record` drops on error, and the LSP `Analyse` path keeps only
  diagnostics + hints.
- **`check_record` bails per-file on any error**, so a receiver is typed only
  when its file otherwise checks clean.

## Decision
Ship value-receiver `.`-member completion via three pieces:

1. **Retain `expr_types` to the analysis** (an `ExprTypeSink` mirroring the
   v0.27 `HintSink`), surfaced on `ProjectDiagnostics`/`ProjectAnalysis`.
   Captured on the **Ok path** — so the **clean-file ceiling** is intrinsic: a
   file with errors records nothing, and completion offers nothing there
   (graceful degradation, the conservative default). Only synthetic files are
   muted — completion runs in test files. A `type_at_offset` query returns the
   innermost expression type covering an offset.
2. **Rewrite on trigger.** A lowercase `receiver.`(`partial`) is a value
   receiver; the LSP rewrites the buffer to drop the trailing `.partial` (so the
   file parses), re-analyses that buffer (the existing `diagnose_project` overlay
   path), and types the receiver via the retained `expr_types`. Re-analysing per
   completion is acceptable (completion is debounced; analyse is the `didChange`
   path) and the ceiling bounds it. An **uppercase** receiver stays the slice-2
   name-receiver path; a decimal `1.` and a `.`-qualified `a.B.` are excluded.
3. **Enumerable kernel registries.** The kernel method names lived in the
   checker's `check_*_kernel_method` `match` arms (authoritative for typing, not
   enumerable). A `kernel_methods` module now lists `(name, signature)` per
   kernel (`List`/`Map`/`Option`/`Result`/`String`/`Int`/`Float`) and a
   `methods_for(&Ty)` mapping. The checker is **untouched** (its
   `method_not_found` messages are golden-tested); a drift test drives every
   listed method through the real checker and asserts none is `method_not_found`
   — so the registry can't list a phantom. Record fields come from the AST.

The member set is the **kernel** methods (method-callable: `xs.fold`, `o.map`,
`s.split`) plus record fields — *not* the `karn.list`/`karn.map` combinators
(`map`/`filter`/…), which are free functions (`map(xs, f)`), reached by
expression-position completion, not member access.

## Consequences
The daily-driver `.`-member completion lands on the proven recovery-parse +
overlay machinery, with the receiver-typing risk bounded by a stated, graceful
ceiling rather than half-built error-tolerant typing. **Locals/params in scope**
remains deferred (slice 4) — it needs a scope-at-offset query the index doesn't
have, independent of this slice. Lifting the clean-file ceiling (error-surviving
or scoped typing) and the combinator/UFCS members are later work.
