---
level: patch
changelog: An `is` receiver that was not lifted to a temp before its bindings were gathered is now a compiler internal error, rather than a `/* TODO: complex is-receiver */` placeholder in the emitted TypeScript that surfaced as a distant `tsc` error. No well-formed program reaches it (#1668)
---
