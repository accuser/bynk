# 0072 — Richer inlay hints: parameter names + generic instantiations

- **Status:** Accepted (v0.39)
- **Spec:** `design/karn-lsp-spec.md` (inlay-hints section)
- **Relates to:** ADR 0056 (the v0.27 inferred-type inlay hints this extends)

## Context
The v0.27 inlay-hint pipeline harvested only **inferred-type** hints (binding and
lambda-param types), recorded as `(span, label)` with the LSP handler hard-coding
`InlayHintKind::TYPE` and an end-anchor. Two high-value hint kinds were missing —
the **parameter name** before each call argument, and the **inferred type
arguments** at a generic call written without them. Both data are already
computed in the checker; the only shared enabler is a hint-kind discriminator.

## Decision
Add a **`HintKind { Type, Parameter }`** to the sink and widen its entry to a
`Hint { span, label, kind }`. The kind drives the LSP rendering:

- **`Type`** → anchor `span.end`, `InlayHintKind::TYPE`, no padding (the label
  carries its own `: ` / `[` separator). Covers both the existing binding-type
  hints *and* generic-instantiation hints (`identity[Int]`).
- **`Parameter`** → anchor `span.start`, `InlayHintKind::PARAMETER`,
  `padding_right` (renders `count: 5`).

**This increment lands parameter-name hints (slice 1).** Generic-instantiation
hints follow in slice 2, reusing the discriminator (a `Type`-kind hint at the
call-name span with a `[A, B]` label — no new sink method).

- **Where recorded:** at the checker's argument loops — free-fn, generic, method,
  and cross-context op/service calls — via a shared `record_param_hint` helper.
  Capability-op *local* dispatch stores params type-only (no names) and is
  skipped; agent-handler dispatch is a minor gap.
- **Suppression:** the helper drops noise — the `_`/`self` placeholders, and an
  argument that **is the identically-named identifier** (`f(count)` for parameter
  `count`), matching rust-analyzer. Literals and complex expressions always get
  the hint.
- **Same toggle, same pipeline:** all hint kinds flow through the existing
  `inlay_hint` provider and the `karn.inlayHints.enable` gate; no new capability.

## Consequences
Parameter names show at calls where they aren't already obvious, materially
improving call-site readability, at the cost of widening `FileHints` (a mechanical
ripple through the sink drain, the LSP `Analysis.hints`, the handler, and the
type-hint test harness, which now filters to `HintKind::Type`). The discriminator
is the foundation slice 2 builds generic-instantiation hints on. The remaining
deferred-polish items (semantic-tokens delta, `inlayHint/resolve`, InRange-swap)
are independent later increments.
