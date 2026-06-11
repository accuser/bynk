# 0042 — Operand-typed division; non-finite arithmetic is host-defined

- **Status:** Accepted (v0.21)
- **Spec:** §5.6, §7.3

## Context
`Int / Int` has truncated since v0.1 (`Math.trunc(a / b)`); `Float`
division must not. With no-coercion (0041) ruling out mixed operands,
the operand type can safely select the lowering.

## Decision
Division lowers by operand type: `Int / Int` stays `Math.trunc(a / b)`
(byte-identical for existing programs); `Float / Float` is bare `a / b`
(true division). The emitter reads the left operand's checked type from
the `expr_types` side table; a missing entry falls back to the
truncating form, so the failure mode preserves v0.20 behaviour.

**Non-finite arithmetic results are host-defined** in v0.21:
`Float` division by zero yields `Infinity`/`NaN` per IEEE 754, with no
Karn-level guard (as `Int` division by zero already follows the host).
Documented normatively; `isNaN`/`isFinite`/checked-division helpers are
v0.22 stdlib calls. The boundary, by contrast, is guarded (0040).

## Consequences
`5 / 2 == 2` and `5.0 / 2.0 == 2.5` both hold, pinned by guard
fixtures. The truncation-regression trap (an operand-typed refactor
dropping `Math.trunc`) is covered by the additive-guard fixture suite.
