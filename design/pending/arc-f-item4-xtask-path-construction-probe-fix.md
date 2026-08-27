---
level: patch
changelog: Fixes a `ts_writes` probe false positive found by Arc F's own item-4 investigation (#1457) — `xtask`'s `is_path_construction_line` now also recognises the `.with_file_name(format!(...))` filesystem-path idiom, alongside the existing `PathBuf::from(format!` and `.join(format!` forms, so `bynk-emit/src/project.rs`'s `sibling_path` is no longer miscounted as TypeScript-producing. `ts_writes` drops from 880 to 879; no other probe changes.
---
