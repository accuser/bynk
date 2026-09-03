---
level: patch
changelog: "xtask's `fs_below_driver`/`ts_writes` gated probes now fail loud in CI if a `NAMED_FS_EXCEPTIONS`/`TS_WRITES_EXCLUDED_FILES` entry outlives the file it names, instead of silently reading a vacuous, falsely-healthy count (#1587)"
---
