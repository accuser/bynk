---
level: patch
changelog: A literal set on a consumed context's refined field (`Item { code: 1, … }` for `ev.store`'s `Code`) is now cast to `ev_store.Code` under the namespace the module already imports, instead of a bare `Code` it never imported, which failed `tsc` with `TS2304`
---
