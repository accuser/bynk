---
level: patch
changelog: "emitter::block_uses_emit now reads the checker's own resolved Callee classification instead of a bare-Ident(\"Events\") receiver name match, closing a real disagreement with project.rs's own unit_table_uses_emit (#1202) on a locally-shadowed Events type that could previously produce TypeScript failing tsc --strict"
---
