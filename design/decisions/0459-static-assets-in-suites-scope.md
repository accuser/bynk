# 0459 — A suite names exactly the assets its target's Worker ships, and fetches them through an HTTP service

- **Status:** Accepted (v0.330.1)

**Context.** Slice 3 of #1883 (#1922, ADR "static-assets-in-suites") let a
case name the assets of every `system` participant and of the suite's own
`uses` commons, while `svc.GET(asset)` answers only from the files the
target's Worker ships. A name that type-checked could therefore answer
`NotFound` with no hint why. The asset fetch was also recorded as a route
address with a sentinel string a real route could collide with, and it was
accepted through any service, not only an HTTP one (review of #1922).

**Decision.**

- A case sees exactly the assets the target's Worker ships, mirroring
  `assets::deployed_assets`: the target's own and its `uses`d commons'
  (content-hashed and fixed-path), plus the content-hashed assets of the
  contexts it consumes (review of #1923). A consumed context's fixed path, or
  an asset of a `system` suite's own `uses` commons, is an ordinary unknown
  name. A unit-tier asset fetch reaches no handler, so it never needs the
  `system` tier for the cross-context calls a handler might make.
- `svc.GET(asset)` is recorded as its own callee, `Callee::TestAssetFetch`.
- `svc` must be a `from http` service; anything else is
  `bynk.test.service_bad_address`.

**Consequences.** Every nameable asset fetches as `Raw(body, contentType)`; a
`NotFound` from `svc.GET(asset)` cannot arise in a program that checks.
