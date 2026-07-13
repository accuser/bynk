# The Cloudflare platform surface — a packaging decomposition

*Input to [`packaging.md`](packaging.md), not a track of its own. Classifies the
Cloudflare platform surface against the packaging model so the first real publish
— the Cloudflare package — **validates** §3–§4 rather than discovering gaps after
the track lands. Realises the intent to build out the Cloudflare package(s) as
soon as the packaging track delivers.*

---

## 1. The gap this fills

Packaging §4.6 splits the **portable ambient** surface (`bynk.time`, `bynk.random`,
`bynk.log`, `bynk.fetch`, `bynk.secrets`) "by ABI coupling", and slice 7 lifts it
post-ABI-freeze. But that split never classifies the **Cloudflare platform
surface** — `consumes bynk.cloudflare { Kv }` today, plus DO storage, queues, cron,
Service Bindings, and WebSockets. Slice 7 says "runtime/bindings stay vendored"
without saying which side of the *package* line `bynk.cloudflare` sits on.

That omission is not a footnote. Every Bynk-on-Cloudflare project consumes this
surface, so `bynk.cloudflare` is the **most-depended-upon package in the
ecosystem** and the flagship dogfood of the packaging system. Its status is the
one the design most needs to get right, which is why it makes the track's natural
acceptance test.

## 2. Two axes, not one

§4.6's "by ABI coupling" is a single axis. The platform surface needs a second,
because "is it ABI-coupled" and "is it a *package*" are different questions:

- **Axis A — portability.** Does the *interface* mean anything off Cloudflare?
  `Clock` does — one interface, N platform bindings. `Kv` does not — it is a
  Cloudflare primitive, and consuming it locks the unit to Cloudflare
  ([ADR 0017](../decisions/0017-platform-lock-per-deployment-unit.md)).
- **Axis B — surface form.** Is it drawn in by `consumes` (a **capability**), or
  lowered from a language keyword (a **construct**)? `Kv` is a consumed capability.
  An agent's `store`, `on queue` / `on cron`, `from websocket`, and cross-context
  `Ref` are constructs — the developer never `consumes` them.

|              | **Portable interface** | **Platform-locked interface** |
|---|---|---|
| **Capability** (`consumes`) | `bynk.time`, `bynk.random`, `bynk.log`, `bynk.fetch`, `bynk.secrets` — interface **published**, binding **vendored** (§4.6) | `bynk.cloudflare` (`Kv`, …) — surface **published** (CF-locked), binding **vendored** ← **the gap** |
| **Construct** (lowered) | *(language triggers/protocols have neutral surface but a per-platform binding)* | DO storage (`store`), `on queue`/`on cron`, `from websocket`, Service Bindings — **never a package**; pure emit ABI, vendored forever |

**Only the capability row yields packages.** The construct row is *never* a
package — its per-platform binding is the emit ABI slice 7 keeps vendored; this
note simply enumerates which bindings those are and why they never appear in
`[dependencies]`.

## 3. The decomposition

**Portable capabilities** (`bynk.time` &c.) — unchanged; §4.6 already covers them.

**The platform capability package — `bynk.cloudflare`:**

- **One cohesive package, flat units — deliberately *not* the per-concern split
  §4.6 chose.** The ambient concerns are independent (`Clock` ⟂ `Fetch`), so they
  became separate packages; the Cloudflare primitives are a *cohesive platform
  surface* that versions with the platform, so one package exposing flat units
  (`Kv` today; the DO-storage surface, a `Queue` producer, an `Analytics` sink as
  each is packaged) is the right grain — and it matches today's
  `consumes bynk.cloudflare { Kv }`. It also exercises the **multi-unit package**
  payload the one-capability-per-package ambient split never does.
- **Surface published, binding vendored** — exactly mirroring `bynk.time`: the
  package ships the capability *declaration* (`Kv` operations, the `putTtl` write
  options, `KvError`) as Bynk source; the Workers-KV glue stays vendored ABI,
  injected at link. The one semantic difference from `bynk.time` is that `Kv`'s
  interface is platform-locked, so the package is *per-platform* — porting means a
  different platform ships a different KV-shaped package, not a second binding
  under one shared interface.
- **npm-free — which is why it is the ideal *first* publish.** The Cloudflare
  built-ins are Workers bindings with no runtime npm module, so `bynk.cloudflare`
  exercises identity, resolution, versioning, and cross-package symbols (§3,
  §4.1–4.4, §4.6) **without** touching npm propagation (§4.5) or adapter trust
  (§7). It isolates the core machinery from the supply-chain machinery — the
  cleanest possible lift-and-shift.
- **Split outliers when they earn it.** A primitive that carries npm/type weight or
  is optional — **D1**, **R2**, Vectorize — splits into its own package so a
  KV-only consumer does not drag its closure. That is §3.1's "the urge to nest is
  the signal to split" applied at the npm-weight boundary; those packages *do*
  exercise §4.5/§7, later.

**Forward map:**

| Package / unit | Today | Kind | Notes |
|---|---|---|---|
| `bynk.cloudflare { Kv }` | ✅ | capability, npm-free | the one shipping surface today |
| `bynk.cloudflare { … }` DO-storage surface, `Queue`, cron surface, `Analytics`/`Tracer` sink | future | capability, npm-free | ride the cohesive package |
| `bynk.cloudflare { Jurisdiction }` | future | capability + construct | **residency / the `region` scope**: `Jurisdiction` surface published here; the DO-address-jurisdiction threading is vendored ABI |
| `bynk.cloudflare-d1`, `bynk.cloudflare-r2` (or platform-prefixed) | future | capability, **npm-carrying** | split out; first to exercise §4.5/§7 for the platform layer |

## 4. Open questions for the track

- **Q10 — the platform dimension vs the flat `org.package.unit` model.**
  `bynk.cloudflare.kv` reads as four parts (org `bynk` / platform `cloudflare` /
  concern `kv` / unit `Kv`), but the identity model (§3.1) has three segments.
  *Leaning:* `cloudflare` is the **package** and primitives are **units**
  (`bynk.cloudflare { Kv, … }`, today's shape); npm-heavy outliers split to sibling
  packages under a flat, platform-prefixed name. A dotted platform *sub-org* is
  outside §3.1's single-segment org rule and is not recommended.
- **Q11 — reserved-org home.** The platform surface stays under reserved org `bynk`
  (§3.1) — `bynk.cloudflare`, never a bare `cloudflare` org. Rationale: reserved-org
  protection (§7 namespace safety) means no third party can publish
  `bynk.cloudflare.*` and impersonate the platform bindings; platform plurality is
  expressed as `bynk.<platform>` packages, not a grabbable top-level org.

## 5. Sample manifest — a Cloudflare SaaS package

```toml
[organisation]
name = "acme"

[package]
name = "flagship"
version = "0.1.0"

[dependencies]
"bynk.cloudflare" = "1.0"    # platform surface — cohesive package, flat units (Kv, …), npm-free, CF-locked
"bynk.time"       = "1.0"    # portable ambient (§4.6)
"bynk.log"        = "1.0"
"acme.money"      = "0.4"    # third-party commons (source mixin)
"acme.stripe"     = "2.1"    # adapter — propagates npm requires (§4.5), reviewed at resolve (§7)
```

## 6. What this asks of the track

1. **Extend the classification** to the platform surface, not just the portable
   ambient (§4.6) — the two-axis model of §2. Landed as `packaging.md` §4.7.
2. **Add Q10 / Q11** to §8.
3. **Note in slice 7** that `bynk.cloudflare` lifts alongside the ambient `bynk.*`
   packages post-ABI-freeze — and, being npm-free, is the lowest-risk member and a
   candidate to *lead* the lift; the npm-carrying outliers (D1, R2) follow under
   §4.5/§7.
