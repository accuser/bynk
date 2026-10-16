# 0471 — The deploy ledger keys a derived KV instance by its binding

- **Status:** Accepted (v0.332.2)

**Context.** ADR 0469 (derived capabilities) put the deploy half of D3 in slice S4b (#1967): a
ledger keyed by (environment, Worker, capability), physical names derived and
environment-qualified, ids minted and never in source. Deploy reads only the generated build
output, never the source (ADR 0193 D3). The generated `wrangler.toml` names an instance by its
binding, `KV_SESSIONS`, not by its capability, `Sessions`. Inverting the binding's screaming snake
case to recover the capability is lossy. A per-Worker manifest carrying the capability names
would be an emitted-code change, and S4b has none.

**Decision.**

- **D1 — Key by binding.** `Environment` gains `kv_instances: BTreeMap<worker,
  BTreeMap<binding, KvNamespace>>`. The binding is a function of the capability's name, and
  #1939 makes it unique per Worker. So it is stable across runs and one-to-one with the
  capability, and a renamed capability is a new key. The map is `default`ed and omitted when
  empty, so `version` stays 1, every older ledger reads, and a project with only the bare `Kv`
  writes the file it wrote before.
- **D2 — The bare instance is unchanged.** The bare `Kv` stays in `kv: BTreeMap<worker, …>`,
  titled with the Worker's name and unqualified under `--env`, as it always was. A live namespace
  is never renamed under an existing project.
- **D3 — Instance titles.** A derived instance's namespace is titled `<worker>-<capability,
  kebab>`, read off the binding (`KV_PRICE_CACHE` → `shop-cart-price-cache`). It is
  environment-qualified like a queue's name (`shop-cart-sessions-staging`). KV titles are
  account-scoped, as Worker names are. The derivation joins two names with the separator Worker
  names already use, so two of a project's namespaces can coincide (Worker `shop` with
  `CartSessions`, Worker `shop-cart` with `Sessions`). `bynk deploy` refuses that before the plan,
  `--dry-run` included, naming both namespaces.
- **D4 — Per binding everywhere.** Each namespace is planned, created, recorded before the push
  (ADR 0180) and checked against the account's live ids on its own. It is materialised into the
  generated config by binding: each entry whose id is still its own placeholder
  (`<KV_SESSIONS_ID>`) gets its id. The CI refusal to mint an unrecorded id applies to every
  namespace. `bynk dev --remote` binds them all.
- **D5 — Orphans.** An instance is an orphan with its Worker, or on its own when its still-live
  Worker no longer declares its binding. The bare namespace likewise, once the live Worker stops
  using the bare `Kv`. A rename is therefore a create plus an orphan. The data
  is not moved, and nothing is deleted without `--prune`'s own confirmation, which names the
  instance's title, Worker and binding. Pruning the last instance of a Worker removes its table
  from the ledger.

**Consequences.**
- `bynk deploy --format json` changes shape. A context's `kv` is a list (`[]` without KV), and an
  `orphans.kv` entry is `{worker, binding?}`. The short plan's existing lines are unchanged. An
  instance's line adds its binding: `kv create shop-cart-sessions (KV_SESSIONS)`, `orphan kv
  shop-cart KV_CARTS`.
- The ledger and the plan name an instance by its binding, not its capability. That name is the
  one the generated config, the `Env` type and the dashboard's binding list show.
- S4a's deploy guard is gone.
