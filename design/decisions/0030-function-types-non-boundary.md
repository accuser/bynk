# 0030 — Function types are confined to non-boundary positions

- **Status:** Accepted (v0.20a)
- **Spec:** §5.8, §6.4a

## Context
Functions cannot serialise: the boundary machinery (`serialise_<T>` /
`deserialise_<T>`, agent persistence) has no representation for them.

## Decision
Function types are legal only in fn/lambda **parameters, returns, and
locals**. They are rejected — `karn.types.function_at_boundary` — in record
fields, sum payloads, service/agent handler signatures, agent state and keys,
and (v0.20a) **capability operation signatures**: a higher-order capability
op is coherent in-process (0008) but enlarges the surface and touches mocks
beyond what this slice needs; it can be allowed additively. Compatibility is
**contravariant in parameters, covariant in return** — the sound
generalisation of refined→base widening.

## Consequences
The serialisation paths carry defensive unreachable arms; the wire format is
untouched; the emitted TS function types are checked by the tsc gate.
