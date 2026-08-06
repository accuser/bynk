---
level: patch
changelog: content-ownership track slice 5 (#1086, #1102) — deletes bynk-emit's read_source disk-read fallback for .bynk project sources, the track's actual target; found and fixed under implementation, three real production-path dependencies on that fallback (bynk-lsp's run_project_diagnostics/type_receiver open-buffers-only overlay, AnalysisRoots::lower's bynk.toml read for every manifest-backed caller, and try_read_project_paths's plain disk-read contract), plus carved out a deliberate, permanent exception for adapter .binding.ts reads (a distinct concern whose path is only known post-parse, so no discovery walk can pre-populate it); adds a real Backend-driven behaviour test proving an unsaved edit in one file is visible to completion in another
---
