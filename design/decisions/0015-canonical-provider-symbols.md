# 0015 — The `bynk` surface names canonical provider symbols

- **Status:** Accepted (v0.17)
- **Spec:** §7.3.6

## Context
Each platform supplies a binding for the `bynk` surface. Either the contract
names the classes every binding must export, or a per-platform manifest maps
platform-chosen names.

## Decision
**Canonical, contract-flavoured symbols** (`ClockProvider`, not `SystemClock`):
every platform's binding exports the same names.

## Consequences
The generated compose is platform-identical — selecting a platform changes only
the imported binding module. No manifest machinery.
