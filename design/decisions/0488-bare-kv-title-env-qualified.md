# 0488 — The bare KV namespace's title is environment-qualified

- **Status:** Accepted (v0.344.1)

**Context.** ADR 0254 D3 said KV needs no environment qualification, having "no account-wide name
to collide on". ADR 0471 D2 kept the bare `Kv` namespace titled with the Worker's name and
unqualified under `--env`, while D3 said KV titles are account-scoped, as Worker names are. The two
could not both be right (#2037).

Cloudflare's API reference for Create a Namespace
(`POST /accounts/{account_id}/storage/kv/namespaces`) settles it: it "Returns `400` if the account
already owns a namespace with that title; an existing namespace must be explicitly deleted before
it can be replaced." Wrangler's `kv namespace create` (`packages/wrangler/src/kv/index.ts`) maps
that error, code 10014, to "A KV namespace with the title … already exists." Titles are unique per
account, so ADR 0471 D3 is right. A first `bynk deploy --env staging` ran
`wrangler kv namespace create <worker>` with the title the default environment already holds, and
failed at provisioning. A default deploy after a staging-first one failed the same way.

Wrangler qualifies the title itself (`<env>-<name>`) when `kv namespace create` is given `--env`.
Bynk never passes it there (ADR 0254 D4), so that prefix never applies.

**Decision.**

- **D1 — Qualify the bare title.** The bare `Kv` namespace is titled
  `env_qualify(environment, worker)`, as Workers, queues and derived instances are. In the default
  environment that is the Worker's name, unchanged, so no existing production namespace is renamed
  or recreated. Under `--env staging` it is `<worker>-staging`.
- **D2 — The title matters only on create.** The ledger already keys the bare namespace's id by
  environment (`environments.<env>.kv.<worker>`). A namespace whose id is recorded for the
  environment, and still live, is reused by id, whatever it was titled when it was made. A staging
  namespace created before this change keeps its title.
- **D3 — Collisions stay refused.** `kv_title_collision` already compares every namespace's
  title within an environment. So under `--env staging`, Worker `shop-cart`'s bare namespace
  (`shop-cart-staging`) and Worker `shop`'s `Cart` instance (also `shop-cart-staging`) are refused
  before the plan, as ADR 0471 D3 describes for two instances.

This amends ADR 0471 D2: the bare instance is unqualified only in the default environment. It
amends ADR 0254 D3's sentence: KV namespace titles are account-wide names that can collide,
although a KV binding's identity is still the driver-minted id. ADR 0471 D3's statement that KV
titles are account-scoped stands.

**Consequences.**

- `bynk deploy --dry-run --env staging` prints `kv create shop-cart-staging` for the bare
  namespace, and `--format json` names it `shop-cart-staging`. Default-environment plans are
  unchanged.
- The plan prints the title Bynk would give a namespace, not the live one. For a staging namespace
  created before this change, `kv reuse shop-cart-staging` names a namespace titled `shop-cart`.
  Derived instances already behave this way. The id is what binds, and looking up the live title
  would add an account read to every plan.
