# 0050 — `Kv.list` is a binding-side drain; cursor-paging deferred

- **Status:** Accepted (v0.23)
- **Spec:** §5.8, §7.3.6, §7.4

## Context
The obvious `list` shape — return a page and a cursor, "caller loops until
`complete`" — is **inexpressible in Bynk today**: `FnDecl` has no `given`
clause (`given` lives only on providers and handlers), capabilities are
not first-class values, and handlers/lambdas cannot self-recurse, so no
recursive or helper-factored routine can hold `Kv`. Plain recursion
compiles; nothing recursive can reach a capability.

## Decision
`fn list(prefix: Option[String]) -> Effect[List[String]]` — the **TS
binding loops the cursor internally** (`env.KV.list({ prefix, cursor })`
until `list_complete`, projecting `keys[].name`) and returns the full key
set. Host iteration inside the host-boundary adapter is the adapter
posture (0016). The drain is **eager and unbounded, normatively** — a very
large namespace loads every matching key; prefer a prefix.

Deferred: cursor-paging (`listPage` + a `KvListPage` record) — a single
page call is expressible, but a *streaming* consumer wants the language
gap fixed first, so the exported type would precede its usable use case.
Also deferred: per-key metadata/expiration on `list` (keys are names
only).

**The forcing limitation is recorded, not fixed**: `given` on free
functions (or first-class capability values) is a language-core increment
of its own. It will recur — paged iteration, retry loops, fan-out —
whenever a capability needs driving from a helper.

## Consequences
The common case ("all keys under a prefix") is writable today and proven
by the first executed adapter-op test (a fake `env.KV` paging at size 2,
asserting the drain crosses page boundaries). When the `given` gap is
fixed, cursor-paging can ship additively without disturbing the drain.
