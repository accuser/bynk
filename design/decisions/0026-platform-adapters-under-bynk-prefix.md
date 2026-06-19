# 0026 — Platform adapters live inside the reserved `bynk.*` prefix

- **Status:** Accepted (v0.19)
- **Spec:** §5.8, §7.3.6

## Context
The retired adapters spec named platform adapters by bare vendor
(`cloudflare`). Shipping one as a synthetic first-party unit would have made
a bare vendor name collide with user units, forcing a new, non-additive
reservation rule per vendor.

## Decision
First-party platform adapters are named **`bynk.<platform>`**
(`bynk.cloudflare`; later `bynk.aws`): inside the prefix the toolchain
already reserves, which decision 0012 anticipated splitting into independent
`bynk.*` units. No new reservation, no break. Supersedes the retired naming
convention "platform adapters by vendor".

## Consequences
One nuance: the `bynk` prefix now means **first-party**, not *portable* — the
surface unit `bynk` remains the portability marker, while `bynk.<platform>`
units are the locked ones. Docs state this explicitly.
