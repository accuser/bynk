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

### Chapter 9: Tests should preserve the architecture

- Suites and cases, capability-scoped stubs, automatic interaction
  observation, the `unit` / `integration` / `system` tier dial, system-tier
  participant inference, and driven agent histories were checked against the
  current testing guides, reference, static semantics, compiler fixtures, and
  representative example suites.
- The successful and rejected programs are manuscript-specific sources under
  `book/snippets/chapter-09/`; none is imported from the online Book.
- The declared project passes `bynkc check` and `bynkc test`, including a
  system-tier cross-context case and a generated history property. The
  conventional comparison passes strict TypeScript checking. The rejected
  project is retained deliberately to exercise `bynk.stub.not_a_seam` when a
  test attempts to introduce a collaborator absent from the target's declared
  capability graph.

### Chapter 10: A compiler refusal can teach the design

- Diagnostic codes, severity, source attribution, notes, structured
  suggestions, project-wide recovery, the generated diagnostic registry, and
  negative-fixture conformance were checked against the current specification,
  compiler implementation, CLI and language-server documentation, and
  diagnostic regression tests.
- The successful and rejected programs are manuscript-specific sources under
  `book/snippets/chapter-10/`; none is imported from the online Book.
- The declared project passes `bynkc check`. The rejected projects are retained
  deliberately to exercise `bynk.resolve.unconsumed_context` for an undeclared
  cross-context call and `bynk.context.consumes_cycle` for the project-wide
  contradiction revealed by adding the missing edge. The warning project
  compiles successfully while reporting `bynk.given.unused_capability`.

### Chapter 11: A new language should not require a new universe

- Typed TypeScript and JavaScript emission, the bundle and workers topologies,
  strict TypeScript conformance, source maps and debug metadata, adapters and
  binding modules, the platform axis, and Cloudflare deployment mappings were
  checked against the current emission and compilation specifications, guides,
  compiler implementation, and representative fixtures.
- The manuscript-specific project under `book/snippets/chapter-11/declared/`
  compiles for both the Node bundle and Cloudflare workers targets. Both emitted
  TypeScript trees pass `tsc --strict`; generated output is inspected but not
  retained in the manuscript source.
- The project's TypeScript binding is copied into both targets and satisfies the
  capability interface emitted from its Bynk adapter. The workers build emits a
  Service Binding between the two contexts and a Durable Object for the agent.
- The platform-lock project passes under the default Cloudflare platform and is
  retained to exercise `bynk.target.vendor_required` when the same source is
  built for Node.

### Chapter 12: Reading a whole system

- The whole-system reading method was checked against the current project
  structure, actors, agents, effects, entry-point, compilation, and deployment
  documentation. The chapter applies those facts to one new manuscript case
  study rather than reusing the documentation's prose or example projects.
- The manuscript-specific order system under
  `book/snippets/chapter-12/whole-system/` contains shared domain values and
  three contexts. It passes `bynkc check` and compiles for both the Node bundle
  and Cloudflare workers targets; both emitted TypeScript trees pass strict
  TypeScript checking.
- The case study deliberately retains two design questions for analysis. A
  failed payment leaves stock reserved because no compensation edge exists,
  and the authenticated order read does not compare caller identity with the
  stored owner. These are valid programs, not compiler-negative fixtures: the
  chapter distinguishes architecture the language can preserve from policy the
  team has not expressed.

### Chapter 13: The cost of stronger constraints

- The costs of acyclic context dependencies, explicit capabilities, keyed state
  ownership, closed failure vocabulary, actor-bearing edges, validated
  boundaries, adapters, and TypeScript emission were checked against the
  current static semantics, reference, and project/deployment guides.
- The chapter distinguishes intrinsic trade-offs from temporary feature gaps.
  In particular, atomic agent commits do not imply cross-agent transactions,
  adapters bound what Bynk can inspect, and the JavaScript/Workers target brings
  operational and organisational dependencies alongside its ecosystem reach.
- The open plugin host under `book/snippets/chapter-13/` is a new
  manuscript-specific TypeScript comparison. It passes strict TypeScript
  checking and represents a genuinely runtime-defined graph, illustrating a
  case where Bynk's compile-visible dependency graph is not the desired model.

### Epilogue: The program should not be able to forget

- The epilogue synthesises the manuscript's existing argument and returns to
  the four-box service introduced in the prologue. It adds no new language
  claims or source examples.
- The closing distinction---a language can preserve a decision but cannot make
  it wise---is grounded in the deliberately valid design defects examined in
  Chapter 12 and the constraint accounting in Chapter 13.
- The final test is intentionally portable beyond Bynk: identify important
  architectural facts that the implementation medium repeatedly erases, then
  choose a proportionate representation and enforcement mechanism.
