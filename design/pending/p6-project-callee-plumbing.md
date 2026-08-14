---
level: patch
changelog: "project.rs's unit_table_uses_emit and called_cross_context_services now read the checker's own already-resolved Callee classification (threaded forward from per-unit checking as a new RunChecks::Checked::unit_callees field) instead of re-deriving Events.emit/cross-context-call detection from raw AST method-call syntax (internal only, byte-identical output)"
---
