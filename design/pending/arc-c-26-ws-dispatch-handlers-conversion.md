---
level: minor
changelog: Arc C, slice 26 — `emit_agent`'s hibernatable-WebSocket dispatch handlers (`webSocketMessage`/`webSocketClose`, via `emit_ws_dispatch_handlers`) convert to real `bynk_ts::TsClassMethod` fragments, built from real `TsStmt`/`TsExpr` nodes rather than one opaque body blob. The second of step (9) sub-slice (5)'s own 3 independent slices; unlike every prior Arc C slice this function lowers no `.bynk` body at all, so no source-map merge was needed — three body statements and several leaf expressions (call/route-arg text, a generic constructor callee, structural parameter types) still stay opaque text, the same established escape hatch this track uses elsewhere. Zero diff on the first attempt.
---
