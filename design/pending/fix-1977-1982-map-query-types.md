---
level: patch
changelog: A `fold` over a collected `store` map's `.entries` no longer names an unimported `MapEntry` type, and `.keys`/`.entries` over a map keyed by an opaque or refined type now yield that branded key type, so both pass `tsc`
---
