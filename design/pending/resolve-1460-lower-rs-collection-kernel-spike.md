---
level: patch
changelog: Resolves #1460 (the `lower.rs` collection-kernel spike) — `distinctBy`/`joinOn`/`leftJoin`/`groupBy` over a `store Query[T]` field (and the held-Map/plain-Map/Log scans lifted into the same query vocabulary) now build their collection buckets typed to the real element type instead of `any[]`/`Record<string, any[]>`, mirroring the `List[T]` sibling kernel's own established pattern (`elem_ts`/`join_other_elem_ts`). `ts_any` drops from 30 to 26. Zero fixture diff except the two fixtures (228, 231) that exercise these exact sites, reblessed and re-verified against real `tsc --strict`.
---
