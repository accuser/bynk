---
level: minor
changelog: "P7.5: carved a new `bynk-ts` crate for the TypeScript tree and printer (depends on `bynk-syntax` only) -- `TsProgram`/`TsStmt::verbatim` (the escape hatch every future Arc C slice converts into real nodes), a printer that owns the buffer end to end, and the companion textual lint over `Verbatim` content (#1307). `bynk-emit`'s source-map machinery relocated unchanged. No `bynk-emit` construction site converts to the tree yet -- Arc C's own first slice starts that. No observable output change."
---
