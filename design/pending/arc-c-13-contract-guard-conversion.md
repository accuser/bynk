---
level: minor
changelog: Arc C, slice 13 — `emit_contract_guarded_body` builds real `bynk_ts` nodes for its precondition/postcondition guards and `return result;` (`bynk_ts` gains `TsStmtKind::Throw`), fully closing step (4) of the design pass's own decomposition order. The result-capturing IIFE stays one deliberately opaque statement (`Pre::lower`'s own predicate lowering is a permanent `emitter/lower.rs` exclusion). Also fixes a second real source-map corruption bug reproducing #1352's own class one level deeper (caught immediately by #1352's own regression test) before it ever landed
---
