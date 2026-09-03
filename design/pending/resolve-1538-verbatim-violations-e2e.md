---
level: patch
changelog: `bynk_ts::verbatim_violations` is wired into the e2e/emit test harness (`bynkc/tests/tsc_verify.rs`), run over every `Verbatim`/`VerbatimExpr` leaf a new `TsProgram::verbatim_content` walker finds by walking each compiled fixture's tree — not trusted by construction. The lint itself gained block-comment and string-literal awareness (it was matching `namespace` inside a message string). Wiring the check up front surfaced two real `(... as any)` violations previously hidden inside opaque `Verbatim` text — fixed at their source: `bynk-emit/runtime/src/boundary.ts` (re-bundled into `runtime.ts`) and `bynk-check/src/firstparty/bindings/bynk-{node,cloudflare}.ts` (#1538)
---
