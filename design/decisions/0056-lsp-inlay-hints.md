# 0056 — Inlay hints are a curated set harvested via a checker sink

- **Status:** Accepted (v0.27)
- **Spec:** `design/karn-lsp-spec.md` §3.13, §4.3

## Context
The checker already computes every binding's type; inlay hints only
need to surface it. But the typed model (`expr_types`) is unusable
as-is for this: it is keyed by expression span (it never contains the
binding-*name* spans a hint anchors to, and cannot say which sites are
annotation-absent without the AST), and it travels in the
`Ok(TypedCommons)` payload `check_record` drops on any error — so
hints carried there would vanish file-wide exactly while the user
types. v0.25's `RefSink` solved the same problem for binding edges:
a `&mut` sink parameter survives the per-file error-`continue`s.

## Decision
A **`HintSink`** (the `RefSink` analogue, `enter_file` attribution
included) threads through `check_record` and
`check_v0_5_declarations`. At each hintable binding — `let` and
`let <-` bindings and lambda parameters whose annotation is absent,
`_` excluded — the checker records `(binding-name span,
": " + final_ty.display())` as it computes the binding's **final**
type. For `let <-` that is the **peeled `Effect[T]` payload**, the
binding's actual type, not the rhs type. `ProjectAnalysis` retains
the per-file `Vec<(Span, String)>` (span-ordered; no `Ty` crosses the
public surface); the LSP's cached `Analysis` keeps it and
`textDocument/inlayHint` filters to the visible range, positions
converting against the analysed snapshot.

Synthetic and test/integration files are muted at `enter_file` (the
`assemble_index` rule). The survives-errors guarantee is bounded to
**sites the checker still reaches**: a fn-body error short-circuits
that file's v0.5 declaration pass, suppressing its handler-body hints
until the error clears.

One deviation from the proposal sketch: no `padding-left` is
requested on the hint — the label's leading `: ` must sit snug
against the binding name to read as source syntax (`x: Int`).

**Deferred:** generic-instantiation hints (`name[?](…)` → the
inferred `[T]`) — the monomorphised type arguments are not stored
queryably; parameter-*name* hints; `inlayHint/resolve` tooltips.

## Consequences
Hints are exactly as correct as the checker (no tool-side
re-inference to drift), persist through transient errors at every
reached site, and cost one small per-file vector per analysis round
rather than retaining the full typed model. Generic-instantiation
hints land for free once the checker records inferred type arguments;
the recording points are the only places that grow.
