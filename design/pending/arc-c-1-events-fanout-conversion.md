---
level: minor
changelog: "Arc C, slice 1: events_fanout.rs's emit_events_fanout_do constructs and returns a real bynk_ts::TsProgram instead of writeln!-ing a String -- the first bynk-emit construction site whose output reaches Document::Ts without a TsStmt::verbatim wrap. Adds TsStmt::Comment (every generated file's header banner needs one) and a multiline field on TsExpr::Object (the __eventRoutes table's own real, multi-line-with-trailing-commas shape, which the existing single-line-only object literal couldn't represent). No output change, verified against the full fixture corpus and a fresh direct test against a real fixture's own events_fanout.ts (#1317)."
---
