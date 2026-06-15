# Versioning & roadmap

Karn is pre-1.0 and is built **spec-first**, in small increments. This page
explains the method and what it means for the docs and for you.

## The spec-first, incremental method

Each language increment (`v0.X`) starts as a written specification, is then
implemented behind a growing fixture suite, and only then is considered done.
Increments are deliberately small: a slice of grammar, a refinement to the type
checker, a new emission detail. This is why the version number moves in fine
steps (the book is written against v0.38) rather than in large releases.

The discipline that keeps it honest is the fixture suite: a large body of
positive examples (which must compile to the expected TypeScript) and negative
examples (which must fail with the expected diagnostic). A feature is not
"shipped" until it is fixtured, and — as of this documentation effort — until its
docs are updated in the same change.

## Document the present

A direct consequence for these docs: **they describe what compiles today.**
Every example in the book is run through the compiler. Where a feature is planned
but not yet shipped, it is marked as planned rather than written as if it exists.
The guiding rule is that aspirational design must never masquerade as current
behaviour.

## What's next

The forward sequence (re-planned after v0.18): the **Cloudflare adapter comes
*before* the standard library**, because a minimal `Kv` (get/put/delete) is
collection-free — it needs only `String`/`Option`/`Effect`, which exist. Each
increment stays single-purpose: language/stdlib work and adapter work never
share an increment (decision record 0023 in `design/decisions/`).

- **v0.19 — Cloudflare `Kv` + lock enforcement (shipped).** The
  `karn.cloudflare` platform adapter (`Kv` get/put/delete);
  `[[kv_namespaces]]` emission and `env.KV` typing derived from first-party
  metadata; platform-lock enforcement live (`karn.target.*`, the effective
  platform computed along the in-process `given` closure).
- **v0.20 — the functional core.** Two slices: **v0.20a (shipped)** —
  first-class functions (lambdas, function types, value application) and
  Open-narrow generic functions; **v0.20b (next)** — built-in `List`/`Map`
  and the Karn-written combinator stdlib. Retires `Fetch`'s missing-headers
  compromise on completion.
- **v0.21 — wider standard library.** JSON/structured values, string/number
  helpers — language/stdlib only; does not touch the adapters.
- **v0.22 — extend `cloudflare`.** With collections and stdlib in place:
  `Kv.list`, structured (JSON) values, and `Queue` (send/sendBatch).
- **Later:** an `aws` platform adapter; more platforms (Deno); shared/singleton
  provider instances; the decorate/wrap override; a public binding ABI.

## What is deferred to v1

Some capabilities are designed but intentionally **deferred, not missing** — they
are scheduled for later increments on the road to v1:

- **Events** — reacting to and emitting domain events.
- **Sagas** — coordinating multi-step workflows across contexts.
- **Storage kinds** — choosing how an agent's state is persisted.

"Deferred, not missing" matters because it shapes how you read the rest of the
book. Their absence is a roadmap decision, not an oversight; when they land, they
will arrive as specified increments with fixtures and docs, exactly like every
feature before them.

## Compatibility during 0.x

While Karn is pre-1.0, increments may change behaviour. The reference
[changelog](../reference/changelog.md) records what changed in each increment and
notes breaking changes. Full multi-version documentation is itself deferred until
the run-up to 1.0, when stability makes it worthwhile; until then the book tracks
a single current version.

## See also

- [Why Karn exists](why-karn-exists.md) — the motivation behind the design these
  increments are building toward.
- Reference: [version compatibility & changelog](../reference/changelog.md).
