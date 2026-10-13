---
level: minor
changelog: 'A suite names its target''s assets and fetches one with `api.GET(style)`, answered as the platform''s asset tier would (`Raw(body, contentType)`, or `NotFound`); `bynk.test.asset_address_method`, `bynk.test.principal_on_asset`; a case now binds an `HttpResult` variant''s payload by its real field (`Raw(body, ct)`, `BadRequest(m)`) (static assets, slice 3)'
---

## ADR: static-assets-in-suites
title: A suite fetches an asset through `svc.GET(asset)`, answered by an imitation of the asset tier
summary: Suites see their target's assets; `svc.GET(asset)` yields `Raw(body, contentType)` from the files the target's Worker ships, at every tier

**Context.** #1883 [DECISION F] chose to make assets testable: a suite should
check that a page links a real asset and that the asset is served. On the
platform an asset is served before the Worker runs, so no handler is involved,
and suites run in-process (`unit`) or against the Worker entry (`system`), not
the platform.

**Decision.**

- **Visibility.** A case sees the assets its target unit sees — its own and
  its `uses`d commons' — plus, in a `system` suite, those of the suite's own
  `uses` commons.
- **The address is an `Asset`** (the reason [DECISION H] chose a type over a
  bare `String`): `svc.GET(style)`, through any HTTP service of the target. It
  is `GET` only (`bynk.test.asset_address_method`), takes the asset alone, and
  carries no `by` (`bynk.test.principal_on_asset`): an asset is public.
- **The answer imitates the asset tier, at every tier.** The test module
  embeds the files the target's Worker ships (`assets::deployed_assets`,
  base64) and answers with `Raw(body, contentType)`, or `NotFound` for a path
  the Worker does not ship. The content type is the one the platform sends for
  the extension, measured against `wrangler dev` 4.107.0
  (`assets::content_type_for`). This is harness data, not the compile-time
  `String` embedding [DECISION E] excludes.
- **Payload bindings.** Matching a service address's result is untyped, and
  every payload binding used to read `.value`, which `tsc` rejected for every
  variant but `Ok`/`Created`/`Accepted`. A binding now reads the runtime's own
  field (`body`/`contentType`, `message`, `location`, `stream`).

**Consequences.** At the `system` tier, a request that a handler route would
receive for an asset's path (`api.GET("/:page", "favicon.ico")`) still reaches
the Worker rather than the asset tier; only `svc.GET(asset)` imitates it.
Routing every system-tier request through the asset tier first would need a
`Raw` decode in `responseToHttpResult`, which does not exist today.
