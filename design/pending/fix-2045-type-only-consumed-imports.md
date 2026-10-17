---
level: patch
probes: ts_writes
changelog: On `workers`, a context imports a consumed context's types type-only (`import { type Receipt }`) and builds that context's sum variants inline (`({ tag: "Held", reason } as Status)`), so the callee's module stays out of the caller's Worker bundle; a consumed type with attached methods still imports as a value
---
