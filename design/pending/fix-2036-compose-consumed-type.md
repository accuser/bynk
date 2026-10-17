---
level: patch
probes: ts_writes
changelog: On `workers`, a consumed context's type on a context's own boundary (`on call(b: Batch)`) type-checks — `handlers.ts` re-exports it as `export { type Batch }` for `compose.ts`'s `handlers.Batch` and generates its codecs even when nothing calls the consumed context
---
