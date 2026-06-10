# 0025 — Platform resources reach bindings via threaded env, on both targets

- **Status:** Accepted (v0.19)
- **Spec:** §7.3.6, §8.5

## Context
KV namespaces exist only on the Worker `env` — not on `globalThis` — so
0021's probe fallback (which covers Secrets) cannot reach them. Restricting
native capabilities to `--target workers` would arbitrarily exclude the
bundle-on-Cloudflare topology (a single Worker with `env` at its entry).

## Decision
Thread `env` on both targets. Workers compose already passes it; **bundle**
`composeApp` gains an **optional `env?: unknown` parameter** — but only when
the program's closure reaches a platform-native unit, so native-free programs
emit the v0.18 parameterless signature byte-identically. The binding reads
`env.KV` explicitly and throws a clear error when absent; in programs that
thread env, env-taking ambient providers (Secrets) receive it too.

## Consequences
Bundle-on-Cloudflare stays a real deployment. The 0021 convention (optional
env ctor param) holds; the difference is platform resources have no probe
fallback, by nature.
