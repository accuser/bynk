---
level: patch
changelog: "A context split across files whose files use one `uses`d commons type builds again (#1820, review of #1852). Each such file exports the context's rebrand of the type and re-exports its codecs, so the context's barrel (`handlers.ts` on workers, `<context>.ts` on bundle, which every bundle build now emits) re-exported the name from two modules through `export *`, which `tsc` rejects as ambiguous (TS2308) and which drops the name at runtime. Each barrel now re-exports a name two or more files export from one of them. On workers, a split context's per-file module is named for its file's stem, with no reserved name"
---
