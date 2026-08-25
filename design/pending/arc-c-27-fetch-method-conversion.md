---
level: minor
changelog: Arc C, slice 27 — `emit_agent`'s workers-mode `fetch` method and `emit_ws_open_fetch_branch` convert to real `bynk_ts::TsClassMethod`/`TsStmt` fragments, closing step (9) sub-slice (5) entirely (all 3 independent slices landed: the WS-hosted DO methods, the hibernatable-WebSocket dispatch handlers, and now the DO dispatch itself). Adds a new fixture pinning the `agent_uses_emit` branch (an agent handler that emits directly, no service in between), which previously had zero fixture coverage anywhere.
---
