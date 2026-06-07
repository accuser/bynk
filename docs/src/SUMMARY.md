# Summary

[Introduction](introduction.md)

# Getting started

- [What is Karn?](introduction/what-is-karn.md)
- [Coming from TypeScript](introduction/coming-from-typescript.md)
- [Install](introduction/install.md)
- [How these docs are organised](introduction/how-these-docs-are-organised.md)

# Tutorials

- [1. Compile your first program](tutorials/01-first-program.md)
- [2. Build a small HTTP service](tutorials/02-http-service.md)
- [3. Model your data with types](tutorials/03-modelling-data.md)
- [4. Make illegal states unrepresentable](tutorials/04-refined-types.md)
- [5. Add a stateful agent](tutorials/05-stateful-agent.md)
- [6. Test it](tutorials/06-testing.md)

# How-to guides

- [How-to guides](how-to/index.md)
  - [Refined types](how-to/refined-types/index.md)
    - [Define and validate untrusted input](how-to/refined-types/define-and-validate.md)
    - [Use a literal where a refined type is expected](how-to/refined-types/literal-admission.md)
  - [Pattern matching](how-to/pattern-matching/index.md)
    - [Pattern-match with `match`](how-to/pattern-matching/match.md)
    - [Narrow and bind with `is`](how-to/pattern-matching/narrow-with-is.md)
  - [Types & values](how-to/types/index.md)
    - [Work with `Result` and optional values](how-to/types/result-and-optionals.md)
    - [Define sum, record, and opaque types](how-to/types/define-types.md)
    - [Consume another context's services](how-to/types/consumes.md)
  - [Agents](how-to/agents/index.md)
    - [Build a stateful agent](how-to/agents/stateful-agent.md)
    - [Model an agent as a state machine](how-to/agents/state-machine.md)
  - [Capabilities](how-to/capabilities/index.md)
    - [Compose a provider from other capabilities](how-to/capabilities/compose-a-provider.md)
    - [Share a capability across contexts](how-to/capabilities/share-across-contexts.md)
  - [HTTP](how-to/http/index.md)
    - [Handle an HTTP request](how-to/http/handle-request.md)
  - [Cron](how-to/cron/index.md)
    - [Run a task on a schedule](how-to/cron/handle-cron-trigger.md)
  - [Queue](how-to/queue/index.md)
    - [Process a queued message](how-to/queue/handle-queue-message.md)
  - [Testing](how-to/testing/index.md)
    - [Write tests and mock collaborators](how-to/testing/write-tests.md)
    - [Test a flow across Workers](how-to/testing/integration.md)
  - [Projects](how-to/projects/index.md)
    - [Lay out a project](how-to/projects/layout.md)
    - [Target Cloudflare Workers](how-to/projects/cloudflare-workers.md)
  - [Editor & tooling](how-to/tooling/index.md)
    - [Format your code with `karn-fmt`](how-to/tooling/format.md)
    - [Set up editor support](how-to/tooling/editor-support.md)
  - [Troubleshooting](how-to/troubleshooting/index.md)
    - [`karn.refine.literal_violates`](how-to/troubleshooting/refine-literal-violates.md)
    - [`karn.agents.non_zeroable_state_field`](how-to/troubleshooting/agents-non-zeroable-state-field.md)
    - [`karn.agents.bad_state_initialiser`](how-to/troubleshooting/agents-bad-state-initialiser.md)
    - [`karn.provider.dependency_cycle`](how-to/troubleshooting/provider-dependency-cycle.md)
    - [`karn.exports.*` cross-context capability errors](how-to/troubleshooting/exports-capability-errors.md)
    - [`karn.types.is_base_mismatch`](how-to/troubleshooting/is-base-mismatch.md)
    - [`karn.mock.*` errors](how-to/troubleshooting/mock-errors.md)
    - [`karn.cron.*` errors](how-to/troubleshooting/cron-errors.md)
    - [`karn.queue.*` errors](how-to/troubleshooting/queue-errors.md)
    - [`karn.integration.*` errors](how-to/troubleshooting/integration-errors.md)

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
  - [HTTP](reference/http.md)
  - [Cron](reference/cron.md)
  - [Queue](reference/queue.md)
  - [Testing](reference/testing.md)
  - [`karn.toml` manifest](reference/manifest.md)
  - [CLI (`karnc`)](reference/cli.md)
  - [Diagnostic index](reference/diagnostics.md)
  - [Emission](reference/emission.md)
  - [Version compatibility & changelog](reference/changelog.md)

# Explanation

- [Explanation](explanation/index.md)
  - [Why Karn exists](explanation/why-karn-exists.md)
  - [Why compile to TypeScript](explanation/why-compile-to-typescript.md)
  - [The type-system philosophy](explanation/type-system-philosophy.md)
  - [The refined-literal admission model](explanation/refined-literal-admission.md)
  - [The agent model](explanation/the-agent-model.md)
  - [The testing philosophy](explanation/testing-philosophy.md)
  - [How a Karn program is shaped](explanation/how-a-karn-program-is-shaped.md)
  - [Versioning & roadmap](explanation/versioning-and-roadmap.md)
  - [Karn compared to TypeScript](explanation/karn-compared-to-typescript.md)

# Specification

- [The Karn Language Specification](spec/index.md)
  - [§1 Scope & conformance](spec/scope.md)
  - [§2 Notation & conventions](spec/conventions.md)

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
