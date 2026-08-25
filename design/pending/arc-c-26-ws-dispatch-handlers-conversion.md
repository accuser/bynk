---
level: minor
changelog: Arc C, slice 26 — `emit_agent`'s hibernatable-WebSocket dispatch handlers (`webSocketMessage`/`webSocketClose`, via `emit_ws_dispatch_handlers`) convert to real `bynk_ts::TsClassMethod` fragments. The second of step (9) sub-slice (5)'s own 3 independent slices; unlike every prior Arc C slice this function lowers no `.bynk` body at all, so both methods convert fully with no opaque carve-out for their own structure — zero diff on the first attempt.
---
