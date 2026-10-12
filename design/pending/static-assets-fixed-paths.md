---
level: minor
changelog: 'An `asset` may be served at a fixed path (`asset favicon = "assets/favicon.ico" at "/favicon.ico"`), checked against the Worker''s routes (`bynk.asset.shadows_route`, the warning `bynk.asset.may_shadow_route`, `bynk.asset.path_conflict`, `bynk.asset.invalid_at_path`); `/_assets/` is a reserved route prefix; a Worker ships the hashed assets of the contexts it consumes (static assets, slice 2)'
---

## ADR: static-assets-fixed-paths
title: Fixed asset paths are checked against routes; hashed assets follow `consumes`
summary: `at "/path"` serves an asset at a fixed URL; a route it shadows is an error, or a warning when parameterised; Workers ship consumed contexts' hashed files

**Context.** Content-hashed URLs (slice 1b) suit files a page links, but some
files are fetched by name by clients that never read a page: `/favicon.ico`,
`/robots.txt` (#1883 [DECISION C]). Platform-served assets are matched before
the Worker, so a fixed path can hide a handler. Separately, a context that
`consumes` another can serve a fragment holding the other's `.url`, which then
resolves against the consuming Worker (raised in the review of #1904).

**Decision.**

- **`asset name = "path" at "/url"`** serves the file at that fixed path, and
  `.url` is that path. `at` is contextual, matched only in this position. The
  path must name one file, need no encoding, and stay out of `/_assets/` and
  `/_bynk/` (`bynk.asset.invalid_at_path`); one Worker serves one file at a
  path (`bynk.asset.path_conflict`).
- **Shadowing is checked per Worker.** Checked against `wrangler dev`
  4.107.0, the platform answers an asset's path for **every** method (a `POST`
  gets its `405`), so a literal route equal to a fixed path is unreachable
  (`bynk.asset.shadows_route`, an error), while a parameterised route that
  matches it only loses that one path (`bynk.asset.may_shadow_route`, a
  warning). Routes have no wildcards, so segment-wise matching is exact.
- **`/_assets/` is a reserved route prefix** (`bynk.http.reserved_prefix`,
  beside `/_bynk/`), so content-hashed files never meet a route.
- **Hashed assets follow `consumes` as well as `uses`** (decided on #1883):
  a Worker ships the content-hashed files of every unit it reaches over
  either, transitively. Hashed names never collide, so this costs only
  duplicated bytes. Fixed-path files follow `uses` only: they belong to the
  root of their own Worker, and shipping a consumed context's would collide
  with this one's.
- **The deploy lock** records every served path, fixed ones included.

**Consequences.** A fixed-path file gets the platform's default caching, not
the year-long immutable header the hashed ones get. Asset references in a
`suite` remain for slice 3 of #1883.
