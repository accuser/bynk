---
level: minor
changelog: A context or commons declares a static file with `asset name = "path"`, a value of the new built-in type `Asset` whose `.url` is a `String`; declarations are checked, and a build refuses them with `bynk.asset.not_yet_emitted` until they can be emitted (static assets, slice 1a)
---

## ADR: static-assets-in-the-language
title: Static assets are declared in the language, as values of a built-in `Asset` type
summary: `asset name = "path"` in a context or commons; the name is an `Asset` whose `.url` is the content-hashed URL

**Context.** A context had no way to ship a static file: a stylesheet or icon
was a string literal returned by a function and served through `Raw`, a Worker
invocation per request with no caching story (#1883, found in
bynk-lang/compat-board). The cheapest fix is a manifest line passed through to
wrangler's `[assets]`, but that leaves assets a deploy detail the compiler
cannot see, while Bynk's position is that the shape of a program — its routes,
its services, what is public — lives in the language.

**Decision.**

- **In the language (#1883 [DECISION A]).** `asset name = "path"` is a
  declaration, so the compiler sees every asset: a reference is checked, a URL
  can be content-hashed, and a route conflict can be reported.
- **Per file (B).** Each declaration names one file and gets its own name and
  hash. Directories may follow in a later slice.
- **Core, not `bynk.cloudflare` (D).** Nothing in the surface names the host;
  only the lowering to wrangler's `[assets]` block is Cloudflare-specific.
- **No compile-time embedding (E).** Reading a file into a `String` is a
  separate feature with separate questions.
- **A commons may declare one (G).** The module that writes the markup owns the
  stylesheet it links to. A unit sees its own assets and those of each commons
  it `uses`, one level, as with functions; a local declaration wins; two used
  commons declaring one name is `bynk.uses.name_conflict`.
- **`Asset` is a built-in type (H).** An asset's name is a value of type
  `Asset`, written in type position like any built-in and reserved as a type
  name. Its one field is `url : String`. A typed value, rather than a bare
  `String`, gives a suite a statically resolved address (`api.GET(style)`,
  [DECISION F], a later slice) and leaves room for fields such as `.integrity`.
  In TypeScript it is the structural `{ readonly url: string }`, so code that
  only names the type needs no runtime support.
- **An `Asset` is not data.** It names a file the platform serves, so it may
  not appear in a record field, sum payload, boundary or agent `store` position
  (`bynk.types.asset_at_boundary`); its `.url` may.
- **The path is checked without the disk.** A path that is empty, absolute, or
  contains a `..` segment is `bynk.asset.unsafe_path`: an asset is public, and
  nothing outside the project may be named. The checker stays disk-free
  (`bynk-wasm` compiles it without a filesystem), so whether the file exists is
  checked when the build threads the file reader in.
- **`asset` is a contextual keyword**, in `RESERVED_CONTEXTUAL` beside `event`
  and `messages`, so it stays usable as an identifier.
- **Checked before it is built.** This increment checks declarations; emission
  (the content hash, the `[assets]` block, `bynk dev`) is the next. Until then
  every build refuses a declaration with `bynk.asset.not_yet_emitted`, rather
  than emitting a Worker whose `.url` references are undefined. Checking
  (`bynk check`, the editor) is unaffected.

**Consequences.** An asset is public by construction, which the spec states
(§5.12). The editor index does not yet record assets (no hover or
go-to-definition on an asset name); that needs its own symbol kind. Assets are
not visible inside a `suite` until the testing slice. Hashed URLs, fixed paths
(`at "/favicon.ico"`, [DECISION C]) and route-conflict checks follow in later
slices of #1883.
