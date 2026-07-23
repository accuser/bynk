---
title: Versioning & roadmap
---
Bynk is pre-1.0 and is built **spec-first**, in small increments. This page
explains the method and what it means for the docs and for you.

## The spec-first, incremental method

Each language increment (`v0.X`) starts as a written specification, is then
implemented behind a growing fixture suite, and only then is considered done.
Increments are deliberately small: a slice of grammar, a refinement to the type
checker, a new emission detail. This is why the version number moves in fine
steps (the book is written against v0.231) rather than in large releases.

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

Forward work is planned as a queue of small, single-purpose increments rather
than a fixed version-by-version schedule — a schedule pinned to version numbers
on this page is exactly what goes stale the moment an increment ships. The live
queues are kept under `design/`: the active proposal in `design/proposals/`, and
the planning backlogs (`bynk-tooling-proposal-queue.md`,
`bynk-refactor-proposal-queue.md`). By theme, the current edges are:

- **Language surface.** Continuing to round out the service, effect, and
  standard-library surface — one single-purpose increment at a time.
  Language/stdlib work and platform-adapter work never share an increment
  (decision record 0023 in `design/decisions/`).
- **Editor tooling.** Deepening the `bynkc-lsp` experience — completion,
  navigation, and diagnostics — and the VS Code extension that surfaces it.
- **Distribution.** Publishing the compiler, grammar, and extension through
  their registries as the project approaches a public 1.0.

The larger capabilities that are designed but intentionally held back are listed
next.

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

While Bynk is pre-1.0, increments may change behaviour. The reference
[changelog](/book/reference/changelog/) records what changed in each increment and
notes breaking changes. Full multi-version documentation is itself deferred until
the run-up to 1.0, when stability makes it worthwhile; until then the book tracks
a single current version.

## See also

- [Why Bynk exists](/book/about/why-bynk-exists/) — the motivation behind the design these
  increments are building toward.
- Reference: [version compatibility & changelog](/book/reference/changelog/).
