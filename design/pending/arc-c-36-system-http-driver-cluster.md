---
level: minor
changelog: Arc C, slice 36 — the system-http driver cluster (`emit_system_http_support` in `tests_emit.rs`) builds real `bynk_ts` fragments, the sixth of `tests_emit.rs`'s own 7 proposed slices (per its dedicated grounding pass) — every `__sysdrive_*` request driver (typed, raw, no-auth, raw+no-auth, wrong-method) and the secrets-bootstrap loop now construct real `TsDecl`/`TsStmt`/`TsExpr` nodes instead of `format!`-built text. No behaviour change — every fixture with a `service ... http` route emits byte-identical TypeScript.
---
