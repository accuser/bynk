---
level: patch
changelog: 'An asset fetch in a suite (`api.GET(style)`) is target-scoped: only the target''s own assets are nameable, it works through a `from http` service only, and it is its own callee, so no route can be mistaken for it (static assets, review of slice 3)'
---

## ADR: static-assets-in-suites-scope
title: A suite names only its target's assets, and fetches them through an HTTP service
summary: Every asset a case can name is one the target's Worker ships; a non-HTTP service cannot fetch one

**Context.** Slice 3 of #1883 (#1922, ADR "static-assets-in-suites") let a
case name the assets of every `system` participant and of the suite's own
`uses` commons, while `svc.GET(asset)` answers only from the files the
target's Worker ships. A name that type-checked could therefore answer
`NotFound` with no hint why. The asset fetch was also recorded as a route
address with a sentinel string a real route could collide with, and it was
accepted through any service, not only an HTTP one (review of #1922).

**Decision.**

- A case sees exactly the assets its target unit sees — its own and its
  `uses`d commons' — every one a file the target's Worker ships. Another
  participant's asset, or one from a `system` suite's own `uses` commons, is
  an ordinary unknown name.
- `svc.GET(asset)` is recorded as its own callee, `Callee::TestAssetFetch`.
- `svc` must be a `from http` service; anything else is
  `bynk.test.service_bad_address`.

**Consequences.** Every nameable asset fetches as `Raw(body, contentType)`; a
`NotFound` from `svc.GET(asset)` cannot arise in a program that checks.
