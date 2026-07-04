# 0166 — `Int` and `Float` literals admit an `_` digit separator, stripped before parsing and preserved in the lexeme for `fmt`

- **Status:** Accepted (v0.142; 2026-07-04)
- **Provenance:** the v0.142 body-limits increment (ADR 0165) introduced `maxBody`
  as a raw `Int` byte count — `1048576`, `26214400` — which is hard to read and
  easy to miscount at a glance. This is a small lexical addition shipped alongside
  it to make large numeric literals legible.
- **Realises:** a purely visual digit-grouping separator for numeric literals,
  language-wide.
- **Relates:** ADR 0165 (the byte counts that motivated it); ADR 0043 (float
  literals — the "store the as-written lexeme" precedent this reuses for `fmt`).

## Context

A large numeric literal — `26214400` — is illegible: a reader cannot tell it is 25
MiB without counting digits. Every mainstream language solves this the same way,
with an underscore digit separator (`26_214_400`), and Bynk's `Float` literals
already carry the machinery this needs: they preserve the as-written lexeme so
`fmt` reproduces the author's spelling (ADR 0043). Motivated by ADR 0165's byte
counts, but the feature is not specific to them — it applies to every `Int` and
`Float` literal.

## Decisions

**D1 — An `_` digit separator between digit groups, for both `Int` and `Float`.**
An `Int` or `Float` literal MAY carry `_` between two digits (`1_048_576`,
`1_000.5`). It MUST fall **between digits**: a leading (`_1`), trailing (`1_`),
doubled (`1__2`), or point-/exponent-adjacent separator is a lexical error. This is
the conventional rule and admits no ambiguity.

**D2 — Strip before parsing; preserve the lexeme for `fmt`.** The separators are
**purely visual** — they are stripped before the numeric value is parsed, so
`1_000` and `1000` denote the same value, and no downstream stage (typing,
refinement admission, emission) sees them. But the **as-written lexeme is
preserved**, so `bynkc fmt` reproduces the author's grouping verbatim rather than
normalising it away — reusing exactly the `Float`-literal lexeme-preservation
mechanism of ADR 0043. Bynk does not impose a canonical grouping (no forced
thousands separators); the author's spelling is authoritative.

## Consequences

- `Int` and `Float` literals accept `_` between digit groups; the value is
  unchanged and `fmt` keeps the grouping.
- No effect on typing, refinement admission, or emitted output for a literal
  written without separators — the addition is purely lexical and backward
  compatible.
- Named follow-on (via ADR 0165): a byte `Size` literal (`1.mb`) would layer a unit
  suffix on top of this legibility, not replace it.
