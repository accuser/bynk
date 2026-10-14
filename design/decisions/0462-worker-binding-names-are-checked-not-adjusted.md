# 0462 — A Worker's binding names are checked for clashes, never adjusted

- **Status:** Accepted (v0.330.8)

**Context.** A Worker's `wrangler.toml` binding names come from separate
derivations that know nothing of each other: the KV namespace's fixed `KV`, a
consumed (or event-subscribing) context's `a.b` → `A_B`, and an agent's
`OrderEntity` → `ORDER_ENTITY` (the events fan-out Durable Object included).
Two of them can produce the same name: an agent `Kv` in a context that consumes
`bynk.cloudflare { Kv }`, or a consumed context `kv`, are both `KV`. Wrangler
accepts the config, and one binding silently hides the other in `env`. The
standard-library track plans more kinds of binding (kind-prefixed names such as
`KV_SESSIONS`), each another derivation that can meet the rest.

**Decision.** Every binding a Worker's `wrangler.toml` declares comes from one
list, which `emit_wrangler_toml` writes and a Workers-build check
(`bynk_check::worker_bindings`) reads. A name that occurs twice is the compile error
`bynk.context.duplicate_binding`, reported at the declaration that brought in
the second binding (an agent, or a `consumes` clause) and naming both sources.
No binding name is renamed, suffixed or otherwise adjusted to avoid a clash:
deployed Workers and their Durable Object namespaces depend on the names, so
the author renames the agent or context instead. A new kind of binding is added
to that one list, and is checked as soon as it is emitted. The Worker's
TypeScript `Env` interface (`emitter::workers`) still derives the same names
alongside it, so a new kind of binding must be added there too.

**Consequences.** A project that built a broken Worker now fails to compile on
`--target workers`; a Bundle build, which declares no bindings, is unchanged.
The check runs in the Workers build only, like the other Workers-only checks,
so the editor (which analyses as a Bundle) does not show it. The derivations
are unchanged, so no emitted output moves.
