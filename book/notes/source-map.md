# Source map

This is a research index, not a reuse plan. Existing documentation supplies
facts, examples, and earlier explanations; manuscript prose should be written
for its own argument and reading rhythm.

| Manuscript concern | Useful repository material | Editorial transformation |
|---|---|---|
| Architectural drift | `site/src/content/docs/book/about/why-bynk-exists.md` | Broaden from motivation page into the book's central problem |
| Program boundaries | `site/src/content/docs/book/guides/program-structure/` | Reframe constructs around information lost in ordinary service code |
| Domain meaning | `site/src/content/docs/book/guides/type-system/` | Build a narrative from shape, identity, validity, and admission |
| Explicit effects | `site/src/content/docs/book/guides/effects-and-capabilities/` | Begin with hidden dependencies and their architectural consequences |
| State ownership | `site/src/content/docs/book/guides/agents-and-state/` | Add concurrency and lifecycle pressure before introducing agents |
| Caller authority | `site/src/content/docs/book/guides/actors/` | Connect identity to the meaning of an operation, not route configuration |
| Testing confidence | `site/src/content/docs/book/guides/testing/philosophy.md` | Expand the critique of alternative test-only architectures |
| Pragmatic runtime | `site/src/content/docs/book/guides/projects-build-and-deployment/why-compile-to-typescript.md` | Treat TypeScript emission as a design trade, not a product feature |
| Case studies | `examples/` and `site/src/content/docs/by-example/projects/` | Select recurring systems; read code directly and write new analysis |
| Exact behaviour | `site/src/content/docs/book/reference/` and `spec/` | Verify claims; leave exhaustive rules online |

## Boundary rules

- Do not import prose from `site/` into the manuscript build.
- Do not maintain the same paragraph in both places.
- Prefer links in planning notes over comments embedded in chapters.
- Read complete examples from their canonical repository files when the book is
  discussing those exact programs.
- Put narrative-specific programs in `book/snippets/` and compile-test them.

## Chapter research record

### Chapter 1: When architecture becomes convention

- The description of contexts and `consumes` was checked against the current
  program-structure guide and compiler fixtures.
- The successful and rejected programs are manuscript-specific sources under
  `book/snippets/chapter-01/`; they are not imported from the online Book.
- The declared project passes `bynkc check`. The undeclared project is retained
  deliberately to exercise `bynk.resolve.unconsumed_context`.

### Chapter 2: A data shape is not a domain model

- Identity, refinement, literal admission, `.of`, and opaque construction were
  checked against the current type-system specification and compiler fixtures.
- The successful and rejected programs are manuscript-specific sources under
  `book/snippets/chapter-02/`; none is imported from the online Book.
- The declared project passes `bynkc check`. The two rejected projects are
  retained deliberately to exercise `bynk.types.argument_mismatch` and
  `bynk.refine.literal_violates`.

### Chapter 3: Failure is part of the contract

- `Result`, `Option`, exhaustive matching, `?`, and direct error embeddings
  were checked against the current type-system specification and compiler
  fixtures.
- The successful and rejected programs are manuscript-specific sources under
  `book/snippets/chapter-03/`; none is imported from the online Book.
- The declared project passes `bynkc check`. The rejected project is retained
  deliberately to exercise `bynk.types.non_exhaustive_match` on a nested
  `Result` error variant.

### Chapter 4: Effects should name their requirements

- `Effect`, capability declarations, handler and provider `given` clauses,
  provider composition, and cross-context capability use were checked against
  the current effects-and-capabilities guides, reference, and compiler
  fixtures.
- The successful and rejected programs are manuscript-specific sources under
  `book/snippets/chapter-04/`; none is imported from the online Book.
- The declared project passes `bynkc check`. The rejected project is retained
  deliberately to exercise `bynk.given.undeclared_capability` when a handler
  uses a capability absent from its `given` clause.

### Chapter 5: State needs an owner

- Agent keys, `store` fields, storage kinds, fresh-state initialisation,
  handler-atomic state commits, target-specific addressing, and rehydration
  validation were checked against the current agents-and-state guides,
  reference, static semantics, compiler fixtures, and accepted design records.
- The successful and rejected programs are manuscript-specific sources under
  `book/snippets/chapter-05/`; none is imported from the online Book.
- The declared project passes `bynkc check`. The rejected project is retained
  deliberately to exercise `bynk.agents.non_zeroable_state_field` for a refined
  cell whose type excludes the implicit zero.

### Chapter 6: State changes are contracts

- Sum-typed state, exhaustive transition handlers, snapshot invariants, step
  invariants, genesis-commit behaviour, and `InvariantViolation` persistence
  semantics were checked against the current agents-and-state guides,
  reference, static semantics, compiler fixtures, runtime tests, and accepted
  design records.
- The successful and rejected programs are manuscript-specific sources under
  `book/snippets/chapter-06/`; none is imported from the online Book.
- The declared project passes `bynkc check`. Runtime checks confirm that failed
  snapshot and step predicates preserve the last committed state. The rejected
  project is retained deliberately to exercise
  `bynk.transition.no_step_reference` when a snapshot claim is misclassified as
  a transition.

### Chapter 7: Who is calling is part of the operation

- Actor declarations, handler `by` clauses, sealed identities, refinement
  actors, HTTP fail-closed behaviour, and cross-context `Caller` identity were
  checked against the current actors guides, reference, static semantics,
  compiler fixtures, and representative examples.
- The successful and rejected programs are manuscript-specific sources under
  `book/snippets/chapter-07/`; none is imported from the online Book.
- The declared project passes `bynkc check`, and the conventional comparison
  passes strict TypeScript checking. The rejected project is retained
  deliberately to exercise `bynk.actor.missing_by_on_http` because an HTTP
  route may not leave public versus authenticated access implicit.

### Chapter 8: Time and messages are architectural boundaries

- HTTP request-response agency, queue acknowledgement and retry, cron scheduled
  time and no-retry behaviour, and WebSocket connection ownership were checked
  against the current entry-point guides, reference, compiler fixtures, and
  representative examples.
- The successful and rejected programs are manuscript-specific sources under
  `book/snippets/chapter-08/`; none is imported from the online Book.
- The declared project passes `bynkc check`, and the conventional comparison
  passes strict TypeScript checking. The rejected project is retained
  deliberately to exercise `bynk.queue.return_not_queue_result`: a domain
  `Result` does not state whether queue infrastructure should acknowledge or
  redeliver a message.
