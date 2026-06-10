# Decision records

The design decisions that shaped Karn's incremental evolution, one record per
language-defining call: `Status / Context / Decision / Consequences`, each
citing the [normative spec](../../docs/src/spec/index.md) section that now
embodies it. Records were harvested from the retired per-increment instalment
documents (see the spec's Appendix B); from v0.19 on, each increment adds its
records here as part of the increment itself.

A record is **immutable once accepted** — a reversal is a new record that
supersedes the old one, the way 0016 supersedes the interim portability tiers.

| # | Decision | Status |
|---|---|---|
| [0001](0001-literal-admission-set.md) | Compile-time literal admission is a narrow, closed set | Accepted (v0.9.4) |
| [0002](0002-cron-queue-slicing.md) | v0.10 ships as two slices; queues are consumer-only | Accepted (v0.10) |
| [0003](0003-inline-state-initialisers.md) | Inline state-field initialisers, not an `init` block | Accepted (v0.11) |
| [0004](0004-static-initialiser-expressions.md) | State initialisers are a closed static-expression set | Accepted (v0.11) |
| [0005](0005-provider-constructor-injection.md) | Provider composition is constructor injection in topo order | Accepted (v0.12) |
| [0006](0006-provider-cycles-rejected.md) | Provider dependency cycles are rejected | Accepted (v0.12) |
| [0007](0007-is-refinement-narrowing.md) | Refinement narrowing reuses `is`, disambiguated at check time | Accepted (v0.13) |
| [0008](0008-cross-context-capabilities-local.md) | Cross-context capabilities wire by local instantiation | Accepted (v0.15) |
| [0009](0009-integration-tests-simulated-wire.md) | Integration tests run a simulated wire in Node | Accepted (v0.16) |
| [0010](0010-adapter-unit-kind.md) | Adapters are a distinct unit kind with no logic | Accepted (v0.17) |
| [0011](0011-consumes-capability-selection.md) | Braced `consumes` flattens capabilities; clashes rejected | Accepted (v0.17) |
| [0012](0012-reserved-karn-surface.md) | The `karn` surface is reserved, flat, ambient-only | Accepted (v0.17/v0.18) |
| [0013](0013-explicit-binding-clause.md) | Explicit `binding` clause; pinned npm deps declared there | Accepted (v0.17) |
| [0014](0014-refined-ids-privileged-constructor.md) | Refined boundary IDs; bindings construct through `.of` | Accepted (v0.17) |
| [0015](0015-canonical-provider-symbols.md) | The `karn` surface names canonical provider symbols | Accepted (v0.17) |
| [0016](0016-no-portable-infrastructure.md) | No portable infrastructure tier | Accepted (v0.17) |
| [0017](0017-platform-lock-per-deployment-unit.md) | Platform lock is per deployment unit | Accepted (v0.17 design) |
| [0018](0018-config-as-capability.md) | Config and IO are capabilities; no `needs` clause | Accepted (v0.17/v0.18) |
| [0019](0019-adapter-dependencies.md) | Adapter-to-adapter dependencies via braced `consumes` + `given` | Accepted (v0.18) |
| [0020](0020-adapter-dependency-trust.md) | Adapter npm-dependency trust policy | **Open** |
| [0021](0021-secrets-env-source.md) | Secrets: optional injected env + `globalThis` probe | Accepted (v0.18) |
| [0022](0022-fetch-minimal-typed-core.md) | Fetch ships a minimal typed core pending sequence types | Accepted (v0.18) |
| [0023](0023-platform-adapter-before-stdlib.md) | The Cloudflare adapter lands before the standard library | Accepted (post-v0.18) |
| [0024](0024-platform-native-via-first-party-metadata.md) | Platform-native marking is first-party metadata, not syntax | Accepted (v0.19) |
| [0025](0025-env-threading-for-platform-resources.md) | Platform resources reach bindings via threaded env, both targets | Accepted (v0.19) |
| [0026](0026-platform-adapters-under-karn-prefix.md) | Platform adapters live inside the reserved `karn.*` prefix | Accepted (v0.19) |
| [0027](0027-lambda-value-syntax.md) | Lambda syntax is `(params) => expr`, the shared value arrow | Accepted (v0.20a) |
| [0028](0028-open-narrow-generics.md) | Generics are Open-narrow: functions only, no bounds | Accepted (v0.20a) |
| [0029](0029-type-argument-inference.md) | Type arguments: argument-directed inference + explicit fallback | Accepted (v0.20a) |
| [0030](0030-function-types-non-boundary.md) | Function types are confined to non-boundary positions | Accepted (v0.20a) |
| [0031](0031-effect-non-storable.md) | `Effect[T]` stays non-storable; `<-` confinement extends to function values | Accepted (v0.20a) |
| [0032](0032-named-functions-as-values.md) | Named functions are values where a function type is expected | Accepted (v0.20a) |
| [0033](0033-closures-over-capabilities.md) | Closures over capabilities; bottom-up lambda effectfulness | Accepted (v0.20a) |
| [0034](0034-collections-hybrid-kernel-stdlib.md) | Collections: thin built-in kernel, Karn-written combinator stdlib | Accepted (v0.20b) |
| [0035](0035-list-map-builtin-immutable.md) | `List`/`Map` built-in, immutable; lowerings, wire format, order | Accepted (v0.20b) |
| [0036](0036-collection-kernel-ops.md) | The collection kernel: `prepend` builder, `fold` + `foldEff` iteration | Accepted (v0.20b) |
| [0037](0037-collection-call-surface.md) | Collection call surface: built-in methods, statics, free combinators | Accepted (v0.20b) |
| [0038](0038-map-value-keyable-keys.md) | `Map` keys are value-keyable only | Accepted (v0.20b) |
| [0039](0039-list-literal-empty-inference.md) | List literal syntax; empty-literal inference; the line rule for `[` | Accepted (v0.20b) |
