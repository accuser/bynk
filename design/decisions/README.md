# Decision records

The design decisions that shaped Bynk's incremental evolution, one record per
language-defining call: `Status / Context / Decision / Consequences`, each
citing the [normative spec](../../docs/src/spec/index.md) section that now
embodies it. Records were harvested from the retired per-increment instalment
documents (see the spec's Appendix B); from v0.19 on, each increment adds its
records here as part of the increment itself.

A record is **immutable once accepted** — a reversal is a new record that
supersedes the old one, the way 0016 supersedes the interim portability tiers.

The table below is **complete by construction**: a CI drift guard (`bynkc`'s
`decisions_index` test, run by the `drift` job) fails if an ADR file has no row
or a row links to no file. Summaries and statuses are curated by hand; the
*presence* of every record is enforced.

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
| [0012](0012-reserved-bynk-surface.md) | The `bynk` surface is reserved, flat, ambient-only | Accepted (v0.17/v0.18) |
| [0013](0013-explicit-binding-clause.md) | Explicit `binding` clause; pinned npm deps declared there | Accepted (v0.17) |
| [0014](0014-refined-ids-privileged-constructor.md) | Refined boundary IDs; bindings construct through `.of` | Accepted (v0.17) |
| [0015](0015-canonical-provider-symbols.md) | The `bynk` surface names canonical provider symbols | Accepted (v0.17) |
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
| [0026](0026-platform-adapters-under-bynk-prefix.md) | Platform adapters live inside the reserved `bynk.*` prefix | Accepted (v0.19) |
| [0027](0027-lambda-value-syntax.md) | Lambda syntax is `(params) => expr`, the shared value arrow | Accepted (v0.20a) |
| [0028](0028-open-narrow-generics.md) | Generics are Open-narrow: functions only, no bounds | Accepted (v0.20a) |
| [0029](0029-type-argument-inference.md) | Type arguments: argument-directed inference + explicit fallback | Accepted (v0.20a) |
| [0030](0030-function-types-non-boundary.md) | Function types are confined to non-boundary positions | Accepted (v0.20a) |
| [0031](0031-effect-non-storable.md) | `Effect[T]` stays non-storable; `<-` confinement extends to function values | Accepted (v0.20a) |
| [0032](0032-named-functions-as-values.md) | Named functions are values where a function type is expected | Accepted (v0.20a) |
| [0033](0033-closures-over-capabilities.md) | Closures over capabilities; bottom-up lambda effectfulness | Accepted (v0.20a) |
| [0034](0034-collections-hybrid-kernel-stdlib.md) | Collections: thin built-in kernel, Bynk-written combinator stdlib | Accepted (v0.20b) |
| [0035](0035-list-map-builtin-immutable.md) | `List`/`Map` built-in, immutable; lowerings, wire format, order | Accepted (v0.20b) |
| [0036](0036-collection-kernel-ops.md) | The collection kernel: `prepend` builder, `fold` + `foldEff` iteration | Accepted (v0.20b) |
| [0037](0037-collection-call-surface.md) | Collection call surface: built-in methods, statics, free combinators | Accepted (v0.20b) |
| [0038](0038-map-value-keyable-keys.md) | `Map` keys are value-keyable only | Accepted (v0.20b) |
| [0039](0039-list-literal-empty-inference.md) | List literal syntax; empty-literal inference; the line rule for `[` | Accepted (v0.20b) |
| [0040](0040-float-distinct-erased-base-type.md) | `Float` is a distinct base type, erased to `number`; finite at the boundary | Accepted (v0.21) |
| [0041](0041-no-numeric-coercion-named-conversions.md) | No implicit `Int`↔`Float` coercion; conversions are value methods | Accepted (v0.21) |
| [0042](0042-operand-typed-division.md) | Operand-typed division; non-finite arithmetic is host-defined | Accepted (v0.21) |
| [0043](0043-float-literals.md) | Float literals: fraction/exponent, digit-both-sides, reject overflow, store the lexeme | Accepted (v0.21) |
| [0044](0044-refinement-over-float.md) | Refinement over `Float`: float bounds, numeric predicates extend, bounds match the base | Accepted (v0.21) |
| [0045](0045-typed-json-codec.md) | The typed JSON codec: compiler-backed, no untyped `Json`; type-app on statics | Accepted (v0.22b) |
| [0046](0046-string-kernel.md) | The string kernel: built-in methods, UTF-16 code units, pinned footguns | Accepted (v0.22a) |
| [0047](0047-jsonerror-compiler-known.md) | `JsonError`: a compiler-known, Bynk-inspectable record | Accepted (v0.22b) |
| [0048](0048-combinators-as-kernel-methods.md) | `Option`/`Result` combinators and numeric helpers are kernel methods | Accepted (v0.22a) |
| [0049](0049-bare-int-boundary-integrality.md) | Bare-`Int` boundary fields validate integrality (wire-contract tightening) | Accepted (v0.22b) |
| [0050](0050-kv-list-drain.md) | `Kv.list` is a binding-side drain; cursor-paging deferred (the `given`-gap) | Accepted (v0.23) |
| [0051](0051-kv-write-options-as-ops.md) | `Kv` write options as distinct camelCase ops (`putTtl`) | Accepted (v0.23) |
| [0052](0052-lsp-project-diagnostics.md) | LSP project-wide diagnostics: non-bailing, overlay-aware, file-attributed | Accepted (v0.24) |
| [0053](0053-lsp-binding-index.md) | The LSP binding index: a reference-table sink at the resolution sites; rename validated by re-analysis + index equality | Accepted (v0.25) |
| [0054](0054-structured-suggestions.md) | Quick-fixes are structured suggestions authored at the diagnosis site; the LSP keys on the diagnostic's span, serves from the cached round | Accepted (v0.26) |
| [0055](0055-index-riders.md) | `workspace/symbol` + `documentHighlight` as `ProjectIndex` queries (`kind` omitted) | Accepted (v0.26) |
| [0056](0056-lsp-inlay-hints.md) | Inlay hints: a curated set harvested via a checker sink (`let`/`let <-`/lambda params; peeled payload; survives errors at reached sites) | Accepted (v0.27) |
| [0057](0057-lsp-semantic-tokens.md) | Semantic tokens read the index over a frozen legend; first-party references via a tokens-only `foreign_refs` side table; `refined` only with a refinement present | Accepted (v0.28) |
| [0058](0058-extension-mirrors-legend.md) | The VS Code extension declares + themes the legend's custom tokens; the names are a cross-component contract enforced by a `bynk-lsp` test parsing `package.json` (excluded from the published tarball) | Accepted (v0.29) |
| [0059](0059-refactor-track-posture.md) | The refactor track: behaviour-preserving, patch-versioned, trunk-based, golden-verified | Accepted (v0.29.1) |
| [0060](0060-named-concern-modules.md) | Split sprawling files into named single-concern modules (flat re-export is fine) | Accepted (v0.29) |
| [0061](0061-completion-sliced-positional-first.md) | Completion is sliced; positional first, lexical contexts, static built-ins | Accepted (v0.30) |
| [0062](0062-completion-member-name-vs-value-receiver.md) | `.`-member completion splits by receiver: name now, typed value later | Accepted (v0.30.1) |
| [0063](0063-completion-value-receiver-typing.md) | Value-receiver `.`-member completion: retained `expr_types`, enumerable kernels | Accepted (v0.30.2) |
| [0064](0064-locals-in-scope.md) | Locals in scope: scope ranges from the checker, resolved per-file | Accepted (v0.31) |
| [0065](0065-signature-help.md) | Signature help: lexical call context, name-vs-value callees, shared renderer | Accepted (v0.32) |
| [0066](0066-codelens-reference-counts.md) | CodeLens reference counts from the index | Accepted (v0.33) |
| [0067](0067-call-hierarchy.md) | Call hierarchy from a preserved-owner call graph | Accepted (v0.34) |
| [0068](0068-implementation-navigation.md) | Implementation navigation from a provides-clause impl graph | Accepted (v0.35) |
| [0069](0069-member-index-kinds.md) | Member index kinds via compound names | Accepted (v0.36) |
| [0070](0070-folding-selection-ranges.md) | Folding & selection ranges from one recovered-AST visitor | Accepted (v0.37) |
| [0071](0071-extension-polish.md) | B-2 extension polish: scaffolds, commands, walkthrough, problem-matcher | Accepted (v0.38) |
| [0072](0072-richer-inlay-hints.md) | Richer inlay hints: parameter names + generic instantiations | Accepted (v0.39) |
| [0073](0073-inrange-swap-quickfix.md) | InRange-swap quick-fix via per-bound spans | Accepted (v0.40) |
| [0074](0074-numeric-tostring.md) | `toString` on the numeric kernel | Accepted (v0.42) |
| [0075](0075-string-interpolation.md) | String interpolation (`\(expr)` holes) | Accepted (v0.43) |
| [0076](0076-feature-track-posture.md) | Far-reaching features run as a feature track: persistent design doc, settle-first, front-loaded ADRs, per-slice proposals | Accepted (posture) |
| [0077](0077-service-protocol-on-header.md) | Service protocol moves to the header (`from <protocol>`); method-builders; `from`-less ⇒ `call`-only | Accepted (v0.44) |
| [0078](0078-queueresult-typed-verdict.md) | `QueueResult` typed queue verdict (`Ack`/`Retry`); the agency rule; cron keeps `Result[(), E]` | Accepted (v0.44) |
| [0079](0079-protocols-closed-set.md) | Protocols are a closed set (transports are open); `protocol` reserved; the protocol-descriptor seam | Accepted (v0.44) |
| [0080](0080-actor-schemes-closed-nominal.md) | Auth schemes are a closed, compiler-known nominal set; actors are nominal contracts (sealed-now-openable) | Accepted (v0.45) |
| [0081](0081-verified-identity-context-sealed.md) | A verified actor identity is a context-sealed value: minted at the seam, threaded service→agent, never re-checked | Accepted (v0.45) |
| [0082](0082-by-clause-verify-then-body-defaults.md) | The `by` clause; two-phase verify-then-body; silent per-protocol default actors; HTTP requires `by` | Accepted (v0.45) |
| [0083](0083-bynk-driver-thin-orchestrator.md) | Introduce the `bynk` driver as a thin orchestrator distinct from `bynkc`; resolution order (override → PATH → sibling); driver↔compiler skew; portable detection | Accepted (v0.46) |
| [0084](0084-doctor-output-exit-contract.md) | The `bynk doctor` output / exit-code contract: capability-grouped checks; bare informational, `--only` gates, `--strict` escalates; `--format short`/`json` pinned | Accepted (v0.46) |
| [0085](0085-bearer-token-jwt-hs256.md) | BearerToken is compiler-generated JWT/HS256; identity is the `sub` claim through a string-constructible identity type; HTTP-only; fail-closed → 401 | Accepted (v0.47) |
| [0086](0086-firstparty-sources-as-files-vendored.md) | First-party sources authored as files (embedded via `include_str!`), vendored not published while part of the emit ABI | Accepted (v0.48) |
| [0087](0087-security-ci-posture.md) | Emitted security boundaries carry a standing behavioral bypass-class test; CodeQL SAST + npm audit committed; native secret-scanning push protection | Accepted (v0.49) |
| [0088](0088-optional-by-binder.md) | The `by` binder is optional (`by <Actor>` for anonymous / verify-and-discard); amends 0082; `_` not admitted as a binder | Accepted (v0.50) |
| [0089](0089-signature-hmac-sha256-webhooks.md) | Signature is compiler-generated HMAC-SHA256 over the raw body; configurable header; timestamp-tolerance replay window; HTTP-only, body-required, identity `()` | Accepted (v0.51) |
| [0090](0090-multi-actor-sum-dispatch.md) | Multi-actor handlers are an ordered sum of peer actors, resolved first-wins, keyed by scheme; a sum requires a binder; the body matches the resolved nominal actor; HTTP-only; total failure → 401 | Accepted (v0.52) |
| [0091](0091-authorisation-invariants-refinement-actors.md) | Authorisation invariants are refinement actors over a Bearer base; a closed claim-predicate set checked against the verified claims at the boundary; a failed invariant is 403, distinct from 401; an `Admin` is-a `User` | Accepted (v0.53) |
| [0092](0092-cross-context-caller-value.md) | The cross-context `CallerId` value is the calling context's name, stamped into an `X-Bynk-Caller` header beside the args body, read at the callee boundary and threaded into `deps`; absent caller is fail-closed; static / channel trust, first-party | Accepted (v0.54) |
| [0093](0093-completion-surface-contract.md) | Completion has one canonical *context × candidate-kind* matrix; every populated cell is registry-sourced and complete (coverage-tested); the clean-file ceiling is confined to the value-receiver cell; `.` is a trigger character | Accepted (doc-ADR) |
| [0094](0094-error-tolerant-receiver-typing.md) | Lift the clean-file ceiling by recording best-effort partial `expr_types` in Analyse mode (the checker already computes them; the final `errors.is_empty()` gate withheld them); Build stays Ok-only, so codegen is untouched; monotonic — never worsens | Accepted (slice 4) |
| [0095](0095-unit-source-map.md) | The project analysis exposes `unit_sources` (qualified unit name → project source files), built from non-synthetic parsed files on the structurally-analysed path; the shared enabler for document links and consumed-context navigation that the binding index can't serve | Accepted (slice 6b) |
| [0096](0096-bynk-dev.md) | `bynk dev` builds + serves locally in one step: compile to a managed, gitignored `.bynk/dev/` (cleared each build so stale contexts can't trip selection); select-or-default the context's worker; serve via `wrangler dev` in local mode — no provisioning (the `<KV_NAMESPACE_ID>` placeholder is verified-ignored locally), no `--remote`, `--` passthrough for wrangler flags | Accepted (v0.57) |
| [0097](0097-bynk-new.md) | `bynk new <path>` scaffolds a complete, runnable single-context HTTP service (`bynk.toml` + `.gitignore` + `src/<name>.bynk`) that `bynk dev` serves unmodified — closing the `doctor → new → dev` arc; offline, no-toolchain `std::fs` file-writing; embedded compile-tested template (ADR 0086 precedent); name validated by the real lexer with a `--name` fix-it; refuse-non-empty (ignoring VCS/OS cruft); `.gitignore` is just `/.bynk` | Accepted (v0.58) |
| [0098](0098-bynkc-test-json.md) | `bynkc test --format json` emits a pinned `#[derive(Serialize)]` document (declaration-order = contract) folded from the generated runner's NDJSON (internal protocol, not a consumer surface); three terminal states on a closed `error.kind` (`compile` carries the `$bynkc` diagnostic lines; `runtime` carries a crashed runner's stderr); exit code follows the runner's process status. The `assert` lowering now emits a real project-relative `path:line:col` (was a byte offset), for unit + integration uniformly. A VS Code Test Explorer shells it with click-through; no `bynk-test` crate (a future `bynk test` shells `bynkc`) | Accepted (v0.59) |
| [0099](0099-crate-layering-dependency-direction.md) | `bynkc` decomposes **downward** into a layered, published library set (`bynk-syntax`/`-check`/`-emit`/`-ide`/`-render`); every dependency arrow points down, no upward façades; the five new crates are published lockstep (`version.workspace = true`) | Accepted (crate-decomposition track, slice 0) |
| [0100](0100-structured-data-rendering-separation.md) | Libraries emit structured data and never render; a single shared `bynk-render` crate (over `bynk-syntax::CompileError` **only**) renders for both CLIs identically by construction; the `AttributedError → CompileError` flattening stays above render (no `render → emit` edge); the LSP maps to protocol | Accepted (crate-decomposition track, slice 0) |
| [0101](0101-front-end-links-pipeline-binary-topology.md) | The front-end **links** the library set (no longer shells `bynkc`); a thin `bynkc` binary survives for CI/build determinism + `cargo install bynkc`, `bynk` becomes the human front-end; amends 0083; re-mechanises the deferred v0.59 `bynk test` from shell to link | Accepted (crate-decomposition track, slice 0) |
| [0102](0102-foundation-types-boundary.md) | `span` / `CompileError` / source-cache / the `diagnostics` code registry live in the lowest leaf `bynk-syntax` (keeps the graph acyclic); the now-cross-crate drift tests get fixed homes — `diagnostics_registry` as a workspace integration test, `kernel_registry` in `bynk-ide` dev-depending on `bynk-check` | Accepted (crate-decomposition track, slice 0) |
| [0103](0103-source-map-contract.md) | Source maps are line-level, statement-anchored, emitted as siblings (`.ts.map` + `//# sourceMappingURL`) by `bynk-emit`; the lowering-gap rule anchors every generated line to its enclosing source statement (spike-ratified by stepping `?`/`match` under the V8 inspector — 8→3, 13→6 stops coalesce monotonically); `names` from preserved identifiers; runtime + glue are the skip boundary; emission is unconditional but `sourcesContent`/bundle-chaining are profile-gated so `.bynk` source never reaches a deployed Worker by default | Accepted (debugging track, slice 0) |
| [0104](0104-debug-launch-model.md) | Reuse VS Code's JavaScript debugger via a thin `DebugConfigurationProvider` (compile → start under inspector → hand off); no bespoke Debug Adapter in phase 1 (the map does the relocation; the spike showed glue suffices); targets the Node test runner (`pwa-node`) and workerd (`wrangler dev` CDP, `bynk dev --inspect` fallback); `skipFiles` over toolchain machinery; remote/deployed debugging deferred | Accepted (debugging track, slice 0) |
| [0105](0105-semantic-debug-interposition.md) | The bynk-native semantic layer rewrites js-debug's DAP responses (`variables`/`scopes`/`stackTrace`) **editor-side** — runtime-agnostic, so it reaches workerd (unlike slice 5's in-debuggee generator). Mechanism: a `DebugAdapterTracker` mutates responses in `onDidSendMessage` (spike-confirmed despite the observe-only docs); the cheapest rung. Bounded by a pinned VS Code + a committed regression-guard spike + a rung-2 wrapping-proxy fallback; bespoke DAP-over-CDP stays out. Inference-first (tag/`__`-temps/`deps`), emitter metadata per-slice; binds only to `__bynkChild` sessions, gated by `bynk.debug.semanticValues` | Accepted (semantic-debugging track, slice 0) |
