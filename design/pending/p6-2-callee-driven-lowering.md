---
level: patch
changelog: P6.2 (design/tracks/the-ir.md §6) — bynk-check's Callee gains Store/Query variants, recorded at checker.rs's store-field dispatch ladder (closing R6.5's/R6.12's classification-level defect); bynk-emit::ir::lower now constructs Call/Lambda/Variant driven entirely by Callee, closing R6.10 for these node kinds. No consumer yet and no bynk-emit emission-behaviour change — the production cutover (replacing lower_method_call/lower_call's own TS-emission) is deferred to a future slice, since no IR-to-TypeScript printer exists yet for any node kind (#1143)
---
