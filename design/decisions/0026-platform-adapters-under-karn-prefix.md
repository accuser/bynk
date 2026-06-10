# 0026 — Platform adapters live inside the reserved `karn.*` prefix

- **Status:** Accepted (v0.19)
- **Spec:** §5.8, §7.3.6

## Context
The retired adapters spec named platform adapters by bare vendor
(`cloudflare`). Shipping one as a synthetic first-party unit would have made
a bare vendor name collide with user units, forcing a new, non-additive
reservation rule per vendor.

## Decision
First-party platform adapters are named **`karn.<platform>`**
(`karn.cloudflare`; later `karn.aws`): inside the prefix the toolchain
already reserves, which decision 0012 anticipated splitting into independent
`karn.*` units. No new reservation, no break. Supersedes the retired naming
convention "platform adapters by vendor".

## Consequences
One nuance: the `karn` prefix now means **first-party**, not *portable* — the
surface unit `karn` remains the portability marker, while `karn.<platform>`
units are the locked ones. Docs state this explicitly.
