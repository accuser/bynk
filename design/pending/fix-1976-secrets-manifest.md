---
level: patch
changelog: "`bynk-secrets.json` no longer claims a complete read list it does not have (closes #1976): literal `Secrets.get` names in `provides` bodies — including providers reached through `given` — and in agent handlers are collected into `read`, a Worker that composes an adapter provider `given Secrets` is marked `read_complete: false` because its TypeScript binding's reads cannot be seen, and `bynk deploy`'s `secrets incomplete` line no longer blames a computed name for both"
---
