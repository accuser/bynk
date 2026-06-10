# 0024 — Platform-native marking is first-party metadata, not syntax

- **Status:** Accepted (v0.19)
- **Spec:** §5.8 (the lock rules), §7.3.6

## Context
The lock machinery needs to know which capabilities are platform-native
(lock-inducing). A user-facing `platform "<name>"` marker on adapter
declarations was considered.

## Decision
**First-party metadata** (`firstparty::platform_of(unit)`): the toolchain
registers `karn.cloudflare` as Cloudflare-native. No grammar change; the
env-taking-provider hook (0021) generalises to (unit, provider) keying in the
same stroke. Marker syntax is premature while no user-authored platform
adapters exist — it can be added additively when they become a goal.

## Consequences
§4 is untouched this increment. Env-field typing, `[[kv_namespaces]]`
derivation, and effective-platform computation all read the one metadata
source.
