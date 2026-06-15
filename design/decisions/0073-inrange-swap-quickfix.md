# 0073 — InRange-swap quick-fix via per-bound spans

- **Status:** Accepted (v0.40)
- **Spec:** `design/karn-lsp-spec.md` (code-actions section)
- **Relates to:** ADR 0054 (the `Suggestion` quick-fix mechanism)

## Context
`InRange(lo, hi)` with `lo > hi` raises `karn.types.inverted_range` with a
textual note ("swap the arguments"). The note couldn't be a machine-applicable
fix because the AST kept only the bound **values**, not their source spans — so
the checker couldn't say which text to replace. The numeral spans exist at parse
time and were discarded.

## Decision
Record a **span per `InRange` bound**, then attach a two-edit swap `Suggestion` to
the diagnostic.

- **The gate.** A new `IntBound { value: i64, span: Span }` (`PredKind::InRange(IntBound,
  IntBound)`) and a `span` field on `FloatBound`. `parse_signed_num_literal`
  captures each bound's span, **including a leading `-`** so a signed bound swaps
  as a unit.
- **Int bounds stay value-only — no lexeme.** Integers have one canonical printed
  form, so the formatter prints `{value}` and stays idempotent without a stored
  lexeme. Floats keep their `lexeme` (so `1e10` never normalises to
  `10000000000`) and merely gain the span.
- **The blast radius is mechanical and compiler-enforced.** Changing the variant
  shape turns every reader (checker predicate eval / compatibility / zero-value,
  AST `name()`, emitter codegen, formatter, the unit-test constructors) into a
  compile error until it reads `.value` — so none is missed, and behaviour is
  unchanged. The e2e (byte-stable TypeScript) and `karn-fmt` idempotence fixtures
  are the guard.
- **The fix.** At each `lo > hi` branch, a `MachineApplicable` two-edit suggestion
  swaps the bounds in place — `(lo.span, hi-text)` and `(hi.span, lo-text)`, where
  the text is `value.to_string()` for ints and `lexeme` for floats. The existing
  note stays for non-LSP consumers; the LSP `code_actions` surface turns the
  suggestion into a one-click `CodeAction`.
- **Int and float together** — symmetric; the float variant already carried a
  lexeme and only gains a span.

## Consequences
The inverted-range diagnostic is now one-click fixable. The per-bound spans are a
small, behaviourally inert AST enrichment that other refinement quick-fixes (e.g.
`MinLength > MaxLength`) could reuse. The single real risk — formatter
idempotence — is neutralised by the value-only `IntBound`, covered by the fmt and
e2e fixtures.
