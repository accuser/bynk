---
level: patch
changelog: "In a context split across files, one file can construct an agent another file declares (#1820, bundle target). The sibling import named the types and functions a file used, but not the agent's factory (`__make<Agent>`), so the output failed `tsc` (TS2304). Multi-file contexts on the workers target are still unsupported (each file overwrites the same `handlers.ts`); that is tracked in #1820"
---
