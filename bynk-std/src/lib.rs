//! The Bynk standard library's first-party sources, as data.
//!
//! The `bynk` conformance surface, the Bynk-written commons (`bynk.list`,
//! `bynk.map`, `bynk.string`, `bynk.locale`, `bynk.locale.types`) and the
//! `bynk.cloudflare` platform adapter ship with the compiler rather than being
//! authored by the user. This crate holds their `.bynk` sources, their
//! per-platform TypeScript bindings and the metadata the toolchain reads about
//! them, embedded with `include_str!`. It holds no logic: `bynk-check` parses and
//! injects these sources when a project `uses` or `consumes` them, and provides
//! each binding for the selected [`Platform`]. The `bynk` root namespace is
//! reserved (`bynk.namespace.reserved`) so user code can never collide with it.
//!
//! The sources live under `lib/`: the portable layers in `lib/core/` (with the
//! `bynk` surface's bindings in `lib/core/bindings/`) and the platform adapter in
//! `lib/cloudflare/`. Nothing in `lib/core/` depends on `lib/cloudflare/`, so the
//! platform layer can be split out later without renaming a unit.

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
pub const BYNK_SURFACE_CAPABILITIES: &[&str] = &[
    "Clock",
    "Random",
    "Logger",
    "Fetch",
    "Secrets",
    "Locale",
    "Idempotency",
    "Events",
];

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
///
/// A view over [`UNITS`]: each entry's [`Unit::env_providers`].
pub fn provider_takes_env(unit: &str, provider: &str) -> bool {
    self::unit(unit).is_some_and(|u| u.env_providers.contains(&provider))
}

/// v0.19 decision 0024: which first-party units are **platform-native** —
/// consuming one locks its deployment unit to the returned platform. This is
/// the metadata that drives effective-platform computation, `env` resource
/// typing, and `wrangler.toml` stanza derivation; no user-facing marker
/// syntax exists (additive later, when third-party platform adapters become
/// a goal).
///
/// A view over [`UNITS`]: each entry's [`Unit::platform`].
pub fn platform_of(unit: &str) -> Option<Platform> {
    self::unit(unit).and_then(|u| u.platform)
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
pub const BYNK_LIST_SRC: &str = include_str!("../lib/core/bynk.list.bynk");

/// `bynk.map` — combinators over the `Map` kernel (`empty`, `insert`, `get`,
/// `keys`, `length`). `fromList` is deliberately absent: Bynk has no pair
/// type to spell a `List[(K, V)]` with, so map construction is `Map.empty()`
/// + `insert` (revisit with tuples or generic records).
pub const BYNK_MAP_SRC: &str = include_str!("../lib/core/bynk.map.bynk");

/// Inside the reserved `bynk.*` prefix; injected when `uses`-imported.
pub const STRING_UNIT: &str = "bynk.string";

/// `bynk.string` — Bynk-written helpers over the v0.22a string kernel
/// (`concat`, the `List` `fold`, and the `Option` kernel methods). The
/// kernel itself is compiler built-in (ADR 0046); only derived helpers
/// live here. `join` folds to `Option[String]` so empty-string *elements*
/// are joined faithfully (a bare `""` accumulator could not tell "nothing
/// yet" from "first element was empty").
pub const BYNK_STRING_SRC: &str = include_str!("../lib/core/bynk.string.bynk");

/// Inside the reserved `bynk.*` prefix; injected when `uses`-imported by
/// application code (a message-bundle commons, ADR 0272) — never directly by
/// the `bynk` adapter itself any more (see [`LOCALE_TYPES_UNIT`]).
pub const LOCALE_UNIT: &str = "bynk.locale";

/// `bynk.locale` — the bundle-free `render`/`renderArg` helpers and the
/// `message`/`withText`/`withWhole`/`withNum`/`withMoment` builder API
/// (slice 1: `render` formats `code` plus a deterministic sorted-by-key
/// rendering of `params`; no lookup, no locale-dependent behaviour). `uses
/// bynk.locale.types` for the types its own signatures name.
pub const BYNK_LOCALE_SRC: &str = include_str!("../lib/core/bynk.locale.bynk");

/// Inside the reserved `bynk.*` prefix; injected when `uses`-imported. Also
/// `uses`-imported by the `bynk` adapter itself, to give `capability Locale`'s
/// `current() -> Effect[LocaleTag]` a type to name (an adapter may `uses` a
/// commons; a commons cannot in turn reference an adapter). Split out of
/// `bynk.locale` (locale-negotiation-slice-2 follow-up, #886) specifically so
/// a context can reach `LocaleTag` without also pulling in `bynk.locale`'s
/// own `render` — which previously collided with a message-bundle commons's
/// own synthesised `render` (ADR 0272) the moment a context needed both.
pub const LOCALE_TYPES_UNIT: &str = "bynk.locale.types";

/// `bynk.locale.types` — `LocaleTag`, `MessageArg`, `Message`: a
/// dependency-free leaf (no `uses` of its own) so any consumer, including
/// the `bynk` adapter itself, can reach these types without also reaching
/// `bynk.locale`'s value-level API.
pub const BYNK_LOCALE_TYPES_SRC: &str = include_str!("../lib/core/bynk.locale.types.bynk");

/// The reserved `bynk` conformance-surface adapter (env-free core). It has no
/// `binding` clause — the toolchain supplies one per platform (see
/// [`Platform::bynk_binding_source`]).
pub const BYNK_ADAPTER_SRC: &str = include_str!("../lib/core/bynk.bynk");

/// The Cloudflare binding for the `bynk` surface. Implements the canonical
/// provider symbols against the platform host API. The refined `Uuid` is built
/// through its emitted validating `.of` constructor (§4.4), treating the
/// unreachable `Err` as a bug rather than trusting the value away.
const BYNK_CLOUDFLARE_BINDING: &str = include_str!("../lib/core/bindings/bynk-cloudflare.ts");

/// The Node (≥ the toolchain's `NODE_MAJOR_FLOOR`, in `bynk-emit`) binding for
/// the `bynk` surface (v0.18). Deliberately
/// near-identical to the cloudflare binding: `Date.now`, the global
/// `crypto`/`fetch`, and `console` are the same host API on both runtimes —
/// which is exactly the ambient-surface portability claim (spec §4.2). The
/// `SecretsProvider` reads `process.env` through the same `globalThis` probe
/// (never bare `process`, which would demand @types/node at the tsc gate).
const BYNK_NODE_BINDING: &str = include_str!("../lib/core/bindings/bynk-node.ts");

/// The Browser binding for the `bynk` surface (in-browser track, slice 1). Tier-3:
/// the same capability surface over Web APIs. `Clock`/`Random`/`Logger` are
/// byte-identical to the Node binding (`Date.now`, Web Crypto `crypto.randomUUID`,
/// `console` — all Web standards); the two substitutions are the playground safety
/// boundary — `Fetch` is **withheld** (throws; no outbound egress from the
/// playground origin) and `Secrets` is **unavailable** (throws; a browser has no
/// secret store), both failing loudly rather than silently degrading.
const BYNK_BROWSER_BINDING: &str = include_str!("../lib/core/bindings/bynk-browser.ts");

/// The first-party Cloudflare platform adapter (v0.19): the platform's real
/// infrastructure capabilities, as they are — no portable intersection
/// (decision 0016). The v0.19 surface was the minimal, collection-free `Kv`
/// (decision 0023); v0.23 adds the `list` drain and `putTtl` (0050/0051);
/// structured values are v0.22-codec composition, and `Queue` remains its
/// own future increment. Like the `bynk` surface it has no `binding`
/// clause — the toolchain supplies the binding.
pub const CLOUDFLARE_ADAPTER_SRC: &str = include_str!("../lib/cloudflare/bynk.cloudflare.bynk");

/// Every first-party `.bynk` source, paired with its unit name — the single
/// list every consumer iterates: hover (`describe_firstparty_symbol`),
/// completion (`EMBEDDED_UNITS`), and the parse guard
/// (`every_first_party_source_parses`). Before this list each consumer kept its
/// own hand-maintained copy, and `bynk.locale`/`bynk.locale.types` were added to
/// none of them — so the whole locale surface hovered as nothing and was absent
/// from completion (#901).
///
/// A view over [`UNITS`], in its order, which is observable: hover takes the
/// first source declaring a name, and completion lists units in this order. The
/// `firstparty_sources_cover_every_src_const` drift guard
/// (`bynkc/tests/firstparty_sources.rs`) fails if a `*_SRC` const is left out of
/// [`UNITS`].
pub const FIRSTPARTY_SOURCES: &[(&str, &str)] = &SOURCES;

/// [`FIRSTPARTY_SOURCES`]' backing array, built from [`UNITS`] at compile time.
const SOURCES: [(&str, &str); UNITS.len()] = {
    let mut out = [("", ""); UNITS.len()];
    let mut i = 0;
    while i < UNITS.len() {
        out[i] = (UNITS[i].name, UNITS[i].source);
        i += 1;
    }
    out
};

/// The output path of the Cloudflare platform adapter's binding module,
/// beside the adapter's emitted `bynk/cloudflare.ts` (distinct from the
/// `bynk` *surface*'s per-platform `bynk-cloudflare.ts`).
pub const CLOUDFLARE_BINDING_FILENAME: &str = "bynk/cloudflare.binding.ts";

/// The Cloudflare platform adapter's binding. `WorkersKv` reads the Worker
/// `env` explicitly (decision 0025): KV namespaces exist only on `env` —
/// there is no `globalThis` path — so a missing binding is a clear runtime
/// error rather than a silent fallback.
const CLOUDFLARE_BINDING: &str = include_str!("../lib/cloudflare/cloudflare.binding.ts");

/// The toolchain-supplied binding for the Cloudflare platform adapter.
pub fn cloudflare_binding_source() -> &'static str {
    CLOUDFLARE_BINDING
}

/// What a first-party unit is, and so how a project brings it in: an adapter is
/// injected when a unit `consumes` it, a commons when a unit `uses` it. Local to
/// this crate, which has no dependencies; `bynk-check` maps it onto its own
/// unit kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitKind {
    Adapter,
    Commons,
}

/// The TypeScript binding the toolchain supplies for a first-party unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binding {
    /// A commons: Bynk-written, so nothing to bind.
    None,
    /// One binding per [`Platform`] ([`Platform::bynk_binding_filename`] and
    /// [`Platform::bynk_binding_source`]): the portable `bynk` surface.
    PerPlatform,
    /// A single binding, for a platform-native adapter that runs only on its own
    /// platform (the platform lock rejects every other `--platform`).
    Fixed {
        filename: &'static str,
        source: &'static str,
    },
}

/// One first-party unit, as data (standard-library track, S1 #1945). Adding a
/// unit to the standard library is an entry in [`UNITS`] plus its files; the
/// checker's injection reads this table and needs no change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unit {
    /// The unit's dotted name, inside the reserved `bynk.*` prefix.
    pub name: &'static str,
    /// Whether a project `consumes` it (an adapter) or `uses` it (a commons).
    pub kind: UnitKind,
    /// The synthetic path the injected unit is parsed under. Diagnostics and
    /// emission name the unit by it, so it never changes.
    pub identity_path: &'static str,
    /// The unit's `.bynk` source.
    pub source: &'static str,
    /// The binding the toolchain supplies for it.
    pub binding: Binding,
    /// Its permanent `ExprId` block: the checker parses it at
    /// `FIRSTPARTY_ID_BASE + id_block * FIRSTPARTY_ID_BLOCK`, and injects units
    /// in ascending `id_block` order. A block is never reused or moved, so ids
    /// and injection order stay stable as units are added; a new unit takes the
    /// next free block.
    pub id_block: u32,
    /// The platform that consuming it locks a deployment unit to (decision
    /// 0024), if it is platform-native.
    pub platform: Option<Platform>,
    /// The provider classes whose constructor takes the Worker `env` (decisions
    /// 0021/0025).
    pub env_providers: &'static [&'static str],
}

impl Unit {
    /// The binding module to emit for this unit on `platform`, as
    /// `(output path, source)`, or `None` for a commons.
    pub fn binding_for(&self, platform: Platform) -> Option<(&'static str, &'static str)> {
        match self.binding {
            Binding::None => None,
            Binding::PerPlatform => Some((
                platform.bynk_binding_filename(),
                platform.bynk_binding_source(),
            )),
            Binding::Fixed { filename, source } => Some((filename, source)),
        }
    }
}

/// The standard library's first-party units. Dependencies between them are not
/// listed here: they are the `uses`/`consumes` lines of each unit's own source,
/// which the checker follows when it injects. The table's order is
/// [`FIRSTPARTY_SOURCES`]' order; injection follows [`Unit::id_block`].
pub const UNITS: &[Unit] = &[
    Unit {
        name: BYNK_UNIT,
        kind: UnitKind::Adapter,
        identity_path: "bynk.bynk",
        source: BYNK_ADAPTER_SRC,
        binding: Binding::PerPlatform,
        id_block: 0,
        platform: None,
        env_providers: &["SecretsProvider"],
    },
    Unit {
        name: CLOUDFLARE_UNIT,
        kind: UnitKind::Adapter,
        identity_path: "bynk/cloudflare.bynk",
        source: CLOUDFLARE_ADAPTER_SRC,
        binding: Binding::Fixed {
            filename: CLOUDFLARE_BINDING_FILENAME,
            source: CLOUDFLARE_BINDING,
        },
        id_block: 1,
        platform: Some(Platform::Cloudflare),
        env_providers: &["WorkersKv"],
    },
    Unit {
        name: LIST_UNIT,
        kind: UnitKind::Commons,
        identity_path: "bynk/list.bynk",
        source: BYNK_LIST_SRC,
        binding: Binding::None,
        id_block: 3,
        platform: None,
        env_providers: &[],
    },
    Unit {
        name: MAP_UNIT,
        kind: UnitKind::Commons,
        identity_path: "bynk/map.bynk",
        source: BYNK_MAP_SRC,
        binding: Binding::None,
        id_block: 2,
        platform: None,
        env_providers: &[],
    },
    Unit {
        name: STRING_UNIT,
        kind: UnitKind::Commons,
        identity_path: "bynk/string.bynk",
        source: BYNK_STRING_SRC,
        binding: Binding::None,
        id_block: 4,
        platform: None,
        env_providers: &[],
    },
    Unit {
        name: LOCALE_UNIT,
        kind: UnitKind::Commons,
        identity_path: "bynk/locale.bynk",
        source: BYNK_LOCALE_SRC,
        binding: Binding::None,
        id_block: 6,
        platform: None,
        env_providers: &[],
    },
    Unit {
        name: LOCALE_TYPES_UNIT,
        kind: UnitKind::Commons,
        identity_path: "bynk/locale/types.bynk",
        source: BYNK_LOCALE_TYPES_SRC,
        binding: Binding::None,
        id_block: 5,
        platform: None,
        env_providers: &[],
    },
];

/// The registry entry for `name`, if it is a first-party unit.
pub fn unit(name: &str) -> Option<&'static Unit> {
    UNITS.iter().find(|u| u.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #1958: `WorkersKv` reads the namespace at the name every generated
    /// Worker declares it under (`[[kv_namespaces]] binding`, its `Env`), the
    /// one TypeScript literal no Rust constructor can supply.
    #[test]
    fn the_cloudflare_binding_reads_kv_binding_name() {
        for needle in [
            format!("{{ {KV_BINDING_NAME}?: KVNamespace }}"),
            format!("?.{KV_BINDING_NAME};"),
            format!("(env.{KV_BINDING_NAME})"),
        ] {
            assert!(
                CLOUDFLARE_BINDING.contains(&needle),
                "cloudflare.binding.ts no longer contains `{needle}`"
            );
        }
    }

    /// Each unit's `ExprId` block is permanent: emitted output and the
    /// process-wide parse cache depend on it. These are the blocks the
    /// hand-written injection used before the registry (S1, #1945); a new
    /// unit takes a new block and never moves one of these.
    #[test]
    fn id_blocks_are_pinned_and_distinct() {
        let blocks: Vec<(&str, u32)> = UNITS.iter().map(|u| (u.name, u.id_block)).collect();
        assert_eq!(
            blocks,
            [
                ("bynk", 0),
                ("bynk.cloudflare", 1),
                ("bynk.list", 3),
                ("bynk.map", 2),
                ("bynk.string", 4),
                ("bynk.locale", 6),
                ("bynk.locale.types", 5),
            ]
        );
        let mut sorted: Vec<u32> = UNITS.iter().map(|u| u.id_block).collect();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), UNITS.len(), "two units share an id block");
    }

    /// Hover and completion read [`FIRSTPARTY_SOURCES`] in order, so it is
    /// pinned to the order the hand-written list had.
    #[test]
    fn firstparty_sources_keep_their_order() {
        let names: Vec<&str> = FIRSTPARTY_SOURCES.iter().map(|(n, _)| *n).collect();
        assert_eq!(
            names,
            [
                "bynk",
                "bynk.cloudflare",
                "bynk.list",
                "bynk.map",
                "bynk.string",
                "bynk.locale",
                "bynk.locale.types",
            ]
        );
        for (unit, (name, source)) in UNITS.iter().zip(FIRSTPARTY_SOURCES) {
            assert_eq!((unit.name, unit.source), (*name, *source));
        }
    }

    /// The views over the table answer as the hand-written matches did.
    #[test]
    fn metadata_views_match_the_previous_answers() {
        assert_eq!(platform_of("bynk.cloudflare"), Some(Platform::Cloudflare));
        for name in [
            "bynk",
            "bynk.list",
            "bynk.map",
            "bynk.string",
            "bynk.locale",
            "user.unit",
        ] {
            assert_eq!(platform_of(name), None, "{name}");
        }
        assert!(provider_takes_env("bynk", "SecretsProvider"));
        assert!(provider_takes_env("bynk.cloudflare", "WorkersKv"));
        assert!(!provider_takes_env("bynk", "WorkersKv"));
        assert!(!provider_takes_env("bynk.cloudflare", "SecretsProvider"));
        assert!(!provider_takes_env("bynk", "ClockProvider"));
        assert!(!provider_takes_env("user.unit", "SecretsProvider"));
    }

    /// An adapter carries a binding and a commons does not, and every name and
    /// identity path is unique, every name inside the reserved prefix.
    #[test]
    fn entries_are_well_formed() {
        for u in UNITS {
            assert!(
                u.name == "bynk" || u.name.starts_with("bynk."),
                "{}",
                u.name
            );
            assert_eq!(
                UNITS.iter().filter(|o| o.name == u.name).count(),
                1,
                "{}",
                u.name
            );
            // Two entries under one identity path would be two parsed units
            // that diagnostics and emission cannot tell apart.
            assert_eq!(
                UNITS
                    .iter()
                    .filter(|o| o.identity_path == u.identity_path)
                    .count(),
                1,
                "{}",
                u.identity_path
            );
            match u.kind {
                UnitKind::Adapter => assert_ne!(u.binding, Binding::None, "{}", u.name),
                UnitKind::Commons => {
                    assert_eq!(u.binding, Binding::None, "{}", u.name);
                    assert!(u.env_providers.is_empty(), "{}", u.name);
                }
            }
        }
        assert_eq!(
            unit("bynk.cloudflare").and_then(|u| u.binding_for(Platform::Node)),
            Some((CLOUDFLARE_BINDING_FILENAME, CLOUDFLARE_BINDING))
        );
        assert_eq!(
            unit("bynk").and_then(|u| u.binding_for(Platform::Browser)),
            Some(("bynk-browser.ts", BYNK_BROWSER_BINDING))
        );
        assert_eq!(
            unit("bynk.list").and_then(|u| u.binding_for(Platform::Node)),
            None
        );
    }

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
