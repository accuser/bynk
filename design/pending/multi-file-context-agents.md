---
level: patch
changelog: "A context split across files works on the bundle target when its files declare agents and services (#1820). A file constructing an agent another file declares now imports that file's factory (`__make<Agent>`), where the output failed `tsc` (TS2304). And when two or more files declare agents, or services, the unit's test barrel defines one `__resetAgents`, `__makeSurface` and context deps type over all of them, where each file's own made the barrel ambiguous (TS2308) and left a test's agent reset undefined. Multi-file contexts on the workers target are still unsupported (each file overwrites the same `handlers.ts`); that is tracked in #1820"
---
