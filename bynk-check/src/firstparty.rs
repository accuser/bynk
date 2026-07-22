//! First-party standard adapters embedded in the toolchain (v0.17 §4.2).
//!
//! The `bynk` conformance surface is shipped with the compiler rather than
//! authored by the user. When a project `consumes bynk`, the driver injects the
//! adapter source below as a synthetic unit and provides its binding for the
//! selected [`Platform`]. The `bynk` root namespace is reserved
//! (`bynk.namespace.reserved`) so user code can never collide with it.

/// The deploy platform — a selection axis distinct from the `--target
/// {bundle,workers}` emit mode (§6.2). It chooses which `bynk-<platform>.ts`
/// binding is linked for the `bynk` surface. v0.17 shipped `cloudflare`;
/// v0.18 adds `node`, making the axis observable (and giving v0.19's
/// platform-lock enforcement a second platform to fire against).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Platform {
    #[default]
    Cloudflare,
    Node,
    /// The browser (in-browser track, slice 1 — the REPL/playground host). A
    /// Tier-3 binding exposing the `bynk` surface over Web APIs, composed with
    /// `BuildTarget::Bundle` only (a browser cannot do the Workers wire model).
    Browser,
}

impl Platform {
    /// The output filename of this platform's `bynk` binding module.
    pub fn bynk_binding_filename(self) -> &'static str {
        match self {
            Platform::Cloudflare => "bynk-cloudflare.ts",
            Platform::Node => "bynk-node.ts",
            Platform::Browser => "bynk-browser.ts",
        }
    }

    /// The TypeScript source of this platform's `bynk` binding.
    pub fn bynk_binding_source(self) -> &'static str {
        match self {
            Platform::Cloudflare => BYNK_CLOUDFLARE_BINDING,
            Platform::Node => BYNK_NODE_BINDING,
            Platform::Browser => BYNK_BROWSER_BINDING,
        }
    }

    /// The platform's stable name (for the `--platform` flag and diagnostics).
    pub fn as_str(self) -> &'static str {
        match self {
            Platform::Cloudflare => "cloudflare",
            Platform::Node => "node",
            Platform::Browser => "browser",
        }
    }
}

/// The unit name of the reserved first-party surface adapter.
pub const BYNK_UNIT: &str = "bynk";

/// The capabilities the env-free `bynk` surface exports (its `exports
/// capability { … }` line in `bynk.bynk`). First-party symbols live on
/// synthetic files and are excluded from the binding index, so the
/// unknown-name quick-fix (#852) offering `consumes bynk { … }` needs this
/// explicit list. Kept in sync with the adapter source by
/// `bynk_surface_capabilities_match_the_adapter`.
pub const BYNK_SURFACE_CAPABILITIES: &[&str] =
    &["Clock", "Random", "Logger", "Fetch", "Secrets", "Locale"];

/// The unit name of the first-party Cloudflare platform adapter (v0.19,
/// decision 0026): inside the reserved `bynk.*` prefix, so no separate
/// reservation rule is needed. The surface unit `bynk` stays the portability
/// marker; `bynk.<platform>` units are the platform-locked ones.
pub const CLOUDFLARE_UNIT: &str = "bynk.cloudflare";

/// The fixed Worker binding name for the Kv namespace (decision C1, v0.19):
/// one namespace, one `[[kv_namespaces]]` stanza, one `env.KV` field.
pub const KV_BINDING_NAME: &str = "KV";

/// v0.18 decision 0021 / v0.19 decision 0025: which first-party provider
/// classes take the Worker `env` as a constructor argument, keyed by
/// (unit, provider class). `SecretsProvider` accepts an optional env with a
/// `globalThis` probe fallback; `WorkersKv` *requires* env on use — KV
/// namespaces exist only on the Worker `env`, never on `globalThis`.
pub fn provider_takes_env(unit: &str, provider: &str) -> bool {
    matches!(
        (unit, provider),
        (BYNK_UNIT, "SecretsProvider") | (CLOUDFLARE_UNIT, "WorkersKv")
    )
}

/// v0.19 decision 0024: which first-party units are **platform-native** —
/// consuming one locks its deployment unit to the returned platform. This is
/// the metadata that drives effective-platform computation, `env` resource
/// typing, and `wrangler.toml` stanza derivation; no user-facing marker
/// syntax exists (additive later, when third-party platform adapters become
/// a goal).
pub fn platform_of(unit: &str) -> Option<Platform> {
    match unit {
        CLOUDFLARE_UNIT => Some(Platform::Cloudflare),
        _ => None,
    }
}

/// The unit names of the first-party collection commons (v0.20b): the
/// Bynk-written combinator stdlib over the built-in `List`/`Map` kernel.
/// Inside the reserved `bynk.*` prefix; injected when `uses`-imported.
pub const LIST_UNIT: &str = "bynk.list";
pub const MAP_UNIT: &str = "bynk.map";

/// `bynk.list` — combinators over the `List` kernel (`fold`, `prepend`,
/// `length`, `get`, `foldEff`), written in ordinary Bynk (decision 0034):
/// the first real consumer of v0.20a generics, lambdas, and effectful
/// traversal. Order-preserving combinators build with `fold` + `prepend`
/// and a final `reverse` — O(n) builds, never `append` (which would be
/// O(n²) over the array lowering).
pub const BYNK_LIST_SRC: &str = include_str!("firstparty/bynk.list.bynk");

/// `bynk.map` — combinators over the `Map` kernel (`empty`, `insert`, `get`,
/// `keys`, `length`). `fromList` is deliberately absent: Bynk has no pair
/// type to spell a `List[(K, V)]` with, so map construction is `Map.empty()`
/// + `insert` (revisit with tuples or generic records).
pub const BYNK_MAP_SRC: &str = include_str!("firstparty/bynk.map.bynk");

/// Inside the reserved `bynk.*` prefix; injected when `uses`-imported.
pub const STRING_UNIT: &str = "bynk.string";

/// `bynk.string` — Bynk-written helpers over the v0.22a string kernel
/// (`concat`, the `List` `fold`, and the `Option` kernel methods). The
/// kernel itself is compiler built-in (ADR 0046); only derived helpers
/// live here. `join` folds to `Option[String]` so empty-string *elements*
/// are joined faithfully (a bare `""` accumulator could not tell "nothing
/// yet" from "first element was empty").
pub const BYNK_STRING_SRC: &str = include_str!("firstparty/bynk.string.bynk");

/// Inside the reserved `bynk.*` prefix; injected when `uses`-imported. Also
/// `uses`-imported by the `bynk` adapter itself, to give `capability Locale`'s
/// `current() -> Effect[LocaleTag]` a type to name (an adapter may `uses` a
/// commons; a commons cannot in turn reference an adapter, which is why
/// `LocaleTag`/`Message`/`MessageArg`/`render` live here rather than in
/// `bynk.bynk` alongside `Locale` — Locale capability track, slice 1, #844).
pub const LOCALE_UNIT: &str = "bynk.locale";

/// `bynk.locale` — the locale value types (`LocaleTag`, `MessageArg`,
/// `Message`) and the bundle-free `render` helper (slice 1: formats `code`
/// plus a deterministic sorted-by-key rendering of `params`; no lookup, no
/// locale-dependent behaviour).
pub const BYNK_LOCALE_SRC: &str = include_str!("firstparty/bynk.locale.bynk");

/// The reserved `bynk` conformance-surface adapter (env-free core). It has no
/// `binding` clause — the toolchain supplies one per platform (see
/// [`Platform::bynk_binding_source`]).
pub const BYNK_ADAPTER_SRC: &str = include_str!("firstparty/bynk.bynk");

/// The Cloudflare binding for the `bynk` surface. Implements the canonical
/// provider symbols against the platform host API. The refined `Uuid` is built
/// through its emitted validating `.of` constructor (§4.4), treating the
/// unreachable `Err` as a bug rather than trusting the value away.
const BYNK_CLOUDFLARE_BINDING: &str = include_str!("firstparty/bindings/bynk-cloudflare.ts");

/// The Node (≥ [`NODE_MAJOR_FLOOR`](crate::NODE_MAJOR_FLOOR)) binding for the
/// `bynk` surface (v0.18). Deliberately
/// near-identical to the cloudflare binding: `Date.now`, the global
/// `crypto`/`fetch`, and `console` are the same host API on both runtimes —
/// which is exactly the ambient-surface portability claim (spec §4.2). The
/// `SecretsProvider` reads `process.env` through the same `globalThis` probe
/// (never bare `process`, which would demand @types/node at the tsc gate).
const BYNK_NODE_BINDING: &str = include_str!("firstparty/bindings/bynk-node.ts");

/// The Browser binding for the `bynk` surface (in-browser track, slice 1). Tier-3:
/// the same capability surface over Web APIs. `Clock`/`Random`/`Logger` are
/// byte-identical to the Node binding (`Date.now`, Web Crypto `crypto.randomUUID`,
/// `console` — all Web standards); the two substitutions are the playground safety
/// boundary — `Fetch` is **withheld** (throws; no outbound egress from the
/// playground origin) and `Secrets` is **unavailable** (throws; a browser has no
/// secret store), both failing loudly rather than silently degrading.
const BYNK_BROWSER_BINDING: &str = include_str!("firstparty/bindings/bynk-browser.ts");

/// The first-party Cloudflare platform adapter (v0.19): the platform's real
/// infrastructure capabilities, as they are — no portable intersection
/// (decision 0016). The v0.19 surface was the minimal, collection-free `Kv`
/// (decision 0023); v0.23 adds the `list` drain and `putTtl` (0050/0051);
/// structured values are v0.22-codec composition, and `Queue` remains its
/// own future increment. Like the `bynk` surface it has no `binding`
/// clause — the toolchain supplies the binding.
pub const CLOUDFLARE_ADAPTER_SRC: &str = include_str!("firstparty/bynk.cloudflare.bynk");

/// The output path of the Cloudflare platform adapter's binding module,
/// beside the adapter's emitted `bynk/cloudflare.ts` (distinct from the
/// `bynk` *surface*'s per-platform `bynk-cloudflare.ts`).
pub const CLOUDFLARE_BINDING_FILENAME: &str = "bynk/cloudflare.binding.ts";

/// The Cloudflare platform adapter's binding. `WorkersKv` reads the Worker
/// `env` explicitly (decision 0025): KV namespaces exist only on `env` —
/// there is no `globalThis` path — so a missing binding is a clear runtime
/// error rather than a silent fallback.
const CLOUDFLARE_BINDING: &str = include_str!("firstparty/bindings/cloudflare.binding.ts");

/// The toolchain-supplied binding for the Cloudflare platform adapter.
pub fn cloudflare_binding_source() -> &'static str {
    CLOUDFLARE_BINDING
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hardcoded [`BYNK_SURFACE_CAPABILITIES`] must match the adapter's
    /// `exports capability { … }` line — otherwise the unknown-name quick-fix
    /// (#852) would offer a stale `consumes bynk { … }` set.
    #[test]
    fn bynk_surface_capabilities_match_the_adapter() {
        let src = BYNK_ADAPTER_SRC;
        let line = src
            .lines()
            .find(|l| l.trim_start().starts_with("exports capability {"))
            .expect("adapter declares `exports capability`");
        let inner = line
            .split_once('{')
            .and_then(|(_, r)| r.split_once('}'))
            .map(|(names, _)| names)
            .expect("braced capability list");
        let declared: Vec<&str> = inner.split(',').map(|s| s.trim()).collect();
        assert_eq!(
            declared, BYNK_SURFACE_CAPABILITIES,
            "BYNK_SURFACE_CAPABILITIES is out of sync with bynk.bynk"
        );
    }
}
