---
level: patch
changelog: "A boundary handler parameter whose name is a JavaScript reserved word (`class`, `void`, `public`, `static`, `delete`, …) no longer emits an invalid `const class = …` binder that breaks the Worker build — the entry-point and compose wrappers now route every such binder through `ts_ident`, exactly as the surface already did (#723)."
---
