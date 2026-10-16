# 0469 — Derived capabilities name context-owned platform resources

- **Status:** Accepted (v0.332)

**Context.** A Worker can bind any number of KV namespaces, D1 databases, R2 buckets and queues, but
Bynk offered one of each kind: `consumes bynk.cloudflare { Kv }` gave a context exactly one namespace,
bound as `KV`. Two kinds of data in one context had to share a namespace and partition its keys by
hand. The standard-library track (spine #1936) settled the model as D3 after working through thirteen
scenarios (track Appendix A). It considered a `resource` keyword, `provides Sessions = Kv` and type
arguments, and chose a derived capability. Slice S4a (#1966) builds it for KV. S4b (#1967) builds
deploy provisioning.

**Decision.**

- **D1 — Meaning.** `capability X = K` declares a capability `X` whose operations are `K`'s. Each
  declaration is a **distinct resource instance**, not an alias. Two derivations of `Kv` are two
  namespaces. This is the meaning `=` already has: `type Id = Int` is a distinct nominal type over
  `Int`. Hover says "a distinct instance of `Kv`".
- **D2 — Implied provision.** No `provides` is written. `X` is provided by `K`'s platform provider
  (`WorkersKv`), bound to `X`'s own resource. `provides` keeps its one meaning: an existing capability
  on the left, a new implementation on the right. `provides Sessions = Kv` would have reversed it. In
  every other way `X` is an ordinary capability: `given X`, `X.op(…)`, `stub`, `expect`, deps by name
  and the platform lock all apply unchanged.
- **D3 — Options are annotations.** Creation parameters follow the base as `@annotations`, from a
  closed set per kind (`@queue("…")` for a queue producer, later). An unknown one is
  `bynk.capability.unknown_option`. `Kv` takes none.
- **D4 — Ownership.** An instance belongs to the context that declares it:
  - a commons or adapter cannot declare one (`bynk.capability.derived_outside_context`);
  - a context cannot export one (`bynk.exports.derived_capability`);
  - another context reaches the data through the owning context's services.

  Adapter-declared instances (track D3 rule 5) come with the first adapter that needs one. They can
  be added without breaking anything.
- **D5 — The base.**
  - It must be a capability the unit selects with `consumes U { K }` (`bynk.capability.unknown_base`),
    so the `consumes bynk.cloudflare` line stays the platform marker.
  - It must be a kind with instances (`bynk.capability.base_not_instantiable`). `bynk-std`'s registry
    lists them per unit (`Unit::instantiable`), each with its binding prefix and options. Only
    `bynk.cloudflare`'s `Kv` is instantiable today.
- **D6 — Three names.** The compiler derives the **binding**: the kind's prefix, then the capability
  in screaming snake case (`Sessions` → `KV_SESSIONS`, `PriceCache` → `KV_PRICE_CACHE`). The binding
  must be unique within the Worker (`bynk.context.duplicate_binding`, #1939). `bynk deploy` will
  derive the **physical name** from the Worker and the capability, environment-qualified. The deploy
  ledger will own the minted **id**, keyed by (environment, Worker, capability). Ids never appear in
  source.
- **D7 — The bare base stays the default instance.** A context may use the bare `Kv` alongside its
  instances. Its `KV` binding and stanza are emitted only when the bare `Kv` is used, so a context
  that consumes `Kv` only to derive instances carries no unreachable namespace. Every context that
  uses the bare `Kv` emits byte-identically.
- **D8 — Emission.**
  - An instance's TypeScript type is an alias of its base's (`export type Sessions = bynk_cloudflare.Kv`).
  - Every composer builds the instance as the base's provider with the binding name as one more
    argument (`new WorkersKv(env, "KV_SESSIONS")`): Worker compose, the bundle's `composeApp`, an
    agent's deps, a provider's `given`. `WorkersKv` reads `env[binding]`, defaulting to `KV`.
  - On Workers, each instance gets an `Env` member and a `[[kv_namespaces]]` stanza whose id is its
    own placeholder (`<KV_SESSIONS_ID>`).
- **D9 — Tests.** Under `bynkc test` each instance has its base's in-memory double, its own and fresh
  per case, so instances never share keys. The integration tier binds each instance's binding to its
  own namespace.
- **D10 — The S4a/S4b split.** S4a ships the language and the build. Until S4b provisions instances,
  `bynk deploy` and `bynk dev --remote` refuse a Worker whose `wrangler.toml` declares any KV binding
  other than `KV`, naming the binding. A refusal is better than sending Cloudflare a placeholder id.
  Local `bynk dev` and `bynkc test` work in full.

**Consequences.**
- A write to an instance commits as it returns and is not rolled back on a fault, like every
  platform-resource write (ADR 0109 D4).
- **Tooling:**
  - Hover on an instance names its base, the base's unit and the base's operations.
  - Completion and signature help on `Sessions.` go through the base.
  - The base identifier is recorded as a capability reference, like a bare `given Kv`. First-party
    surfaces are not indexed, so neither has a go-to-definition target yet.
- **D7 needed no change to the bare-`Kv` gate.** The track expected `needs_kv` to fire whenever a
  context's closure reached `bynk.cloudflare`. It already walked only what handlers and providers
  `given`, so it was use-based. S4a keeps instances out of that walk, so no existing fixture's
  output moved.
- **Deferred:**
  - adapter-declared instances (D4);
  - the qualified form (`capability Sessions = bynk.cloudflare.Kv`);
  - type arguments on the derivation (`Kv[Token, Cart]`);
  - a user-written `provides X = Impl { … }` over a derived shape;
  - the other kinds (Queue, D1, R2, … in P1–P5).
