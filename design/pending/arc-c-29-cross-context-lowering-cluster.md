---
level: minor
changelog: Arc C, slice 29 — the cross-context lowering cluster (`lower_workers_cross_context_call`/`param_cast`) builds real `bynk_ts::TsExpr` fragments instead of hand-written strings, closing step (8) of the TypeScript-tree conversion track. No behaviour change — every cross-context service call at the Workers target emits byte-identical TypeScript, including the zero-argument case.
---
