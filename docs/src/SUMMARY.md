# Summary

[Introduction](introduction.md)

# Getting started

- [What is Karn?](introduction/what-is-karn.md)
- [Coming from TypeScript](introduction/coming-from-typescript.md)
- [Install](introduction/install.md)
- [How these docs are organised](introduction/how-these-docs-are-organised.md)

# Learn Karn

- [1. Compile your first program](tutorials/01-first-program.md)
- [2. Build a small HTTP service](tutorials/02-http-service.md)
- [3. Model your data with types](tutorials/03-modelling-data.md)
- [4. Make illegal states unrepresentable](tutorials/04-refined-types.md)
- [5. Add a stateful agent](tutorials/05-stateful-agent.md)
- [6. Test it](tutorials/06-testing.md)

# Guides

- [Guides](guides/index.md)
  - [The type system](guides/type-system/index.md)
    - [The type-system philosophy](guides/type-system/philosophy.md)
    - [The refined-literal admission model](guides/type-system/refined-literal-admission.md)
    - [Define and validate untrusted input](guides/type-system/define-and-validate.md)
    - [Use a literal where a refined type is expected](guides/type-system/use-a-literal.md)
    - [Define sum, record, and opaque types](guides/type-system/define-types.md)
    - [Work with `Result` and optional values](guides/type-system/result-and-optionals.md)
    - [Pattern-match with `match`](guides/type-system/match.md)
    - [Narrow and bind with `is`](guides/type-system/narrow-with-is.md)
  - [Program structure](guides/program-structure/index.md)
    - [How a Karn program is shaped](guides/program-structure/how-a-program-is-shaped.md)
    - [Consume another context's services](guides/program-structure/consume-services.md)
  - [Effects & capabilities](guides/effects-and-capabilities/index.md)
    - [Understand the capability model](guides/effects-and-capabilities/understand-the-capability-model.md)
    - [Compose a provider from other capabilities](guides/effects-and-capabilities/compose-a-provider.md)
    - [Share a capability across contexts](guides/effects-and-capabilities/share-across-contexts.md)
    - [Wrap a library as an adapter](guides/effects-and-capabilities/wrap-a-library.md)
  - [Agents & state](guides/agents-and-state/index.md)
    - [The agent model](guides/agents-and-state/the-agent-model.md)
    - [Build a stateful agent](guides/agents-and-state/stateful-agent.md)
    - [Model an agent as a state machine](guides/agents-and-state/state-machine.md)
  - [Entry points](guides/entry-points/index.md)
    - [Handle an HTTP request](guides/entry-points/http.md)
    - [Run a task on a schedule (cron)](guides/entry-points/cron.md)
    - [Process a queued message](guides/entry-points/queue.md)
  - [Testing](guides/testing/index.md)
    - [The testing philosophy](guides/testing/philosophy.md)
    - [Write tests and mock collaborators](guides/testing/write-tests.md)
    - [Test a flow across Workers](guides/testing/integration.md)
  - [Projects, build & deployment](guides/projects-build-and-deployment/index.md)
    - [Why compile to TypeScript](guides/projects-build-and-deployment/why-compile-to-typescript.md)
    - [Lay out a project](guides/projects-build-and-deployment/layout.md)
    - [Target Cloudflare Workers](guides/projects-build-and-deployment/cloudflare-workers.md)
  - [Editor & tooling](guides/editor-and-tooling/index.md)
    - [Format your code with `karn-fmt`](guides/editor-and-tooling/format.md)
    - [Set up editor support](guides/editor-and-tooling/editor-support.md)

# Reference

- [Reference](reference/index.md)
  - [Syntax & grammar](reference/grammar.md)
    - [Complete grammar (appendix)](reference/grammar-appendix.md)
  - [Keywords](reference/keywords.md)
  - [Glossary](reference/glossary.md)
  - [Type system](reference/types.md)
  - [Refined-type API](reference/refined-types.md)
  - [Operators & built-ins](reference/operators.md)
  - [Agents](reference/agents.md)
  - [Capabilities & providers](reference/capabilities.md)
  - [Adapters](reference/adapters.md)
  - [HTTP](reference/http.md)
  - [Cron](reference/cron.md)
  - [Queue](reference/queue.md)
  - [Testing](reference/testing.md)
  - [`karn.toml` manifest](reference/manifest.md)
  - [CLI (`karnc`)](reference/cli.md)
  - [Diagnostic index](reference/diagnostics.md)
  - [Emission](reference/emission.md)
  - [Version compatibility & changelog](reference/changelog.md)

# Specification

- [The Karn Language Specification](spec/index.md)
  - [§1 Scope & conformance](spec/scope.md)
  - [§2 Notation & conventions](spec/conventions.md)
  - [§3 Lexical grammar](spec/lexical-grammar.md)
  - [§4 Syntactic grammar](spec/syntactic-grammar.md)
  - [§5 Static semantics](spec/static-semantics.md)
  - [§6 The type system](spec/type-system.md)
  - [§7 Meaning by translation](spec/emission.md)
    - [§7.4 The runtime library](spec/runtime-library.md)
  - [§8 Compilation model](spec/compilation-model.md)
  - [§9 Diagnostics](spec/diagnostics.md)
  - [§10 Conformance & test corpus](spec/conformance.md)
  - [§11 Complete grammar](spec/grammar-appendix.md)
  - [Appendix A — Planned features](spec/appendix-planned.md)
  - [Appendix B — Version history](spec/appendix-version-history.md)

# About Karn

- [Why Karn exists](about/why-karn-exists.md)
- [Karn compared to TypeScript](about/karn-compared-to-typescript.md)
- [Versioning & roadmap](about/versioning-and-roadmap.md)

# Troubleshooting

- [Troubleshooting](troubleshooting/index.md)
  - [`karn.refine.literal_violates`](troubleshooting/refine-literal-violates.md)
  - [`karn.agents.non_zeroable_state_field`](troubleshooting/agents-non-zeroable-state-field.md)
  - [`karn.agents.bad_state_initialiser`](troubleshooting/agents-bad-state-initialiser.md)
  - [`karn.provider.dependency_cycle`](troubleshooting/provider-dependency-cycle.md)
  - [`karn.exports.*` cross-context capability errors](troubleshooting/exports-capability-errors.md)
  - [`karn.adapter.*` / binding errors](troubleshooting/adapter-errors.md)
  - [`karn.types.is_base_mismatch`](troubleshooting/is-base-mismatch.md)
  - [`karn.mock.*` errors](troubleshooting/mock-errors.md)
  - [`karn.cron.*` errors](troubleshooting/cron-errors.md)
  - [`karn.queue.*` errors](troubleshooting/queue-errors.md)
  - [`karn.integration.*` errors](troubleshooting/integration-errors.md)

---

# Contributing

- [Contributing to the compiler](contributing/index.md)
  - [Compiler architecture](contributing/architecture.md)
  - [Testing & fixtures](contributing/testing.md)
  - [Working on the docs](contributing/documentation.md)

# Tooling

- [Tooling](tooling/index.md)
  - [`karn-fmt`](tooling/karn-fmt.md)
  - [`karn-lsp`](tooling/karn-lsp.md)
  - [`tree-sitter-karn`](tooling/tree-sitter-karn.md)
  - [`vscode-karn`](tooling/vscode-karn.md)
