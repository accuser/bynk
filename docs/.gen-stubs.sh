#!/usr/bin/env bash
# One-shot generator for the Karn Book stub pages (Phase 0 scaffold).
# Real prose pages (introduction set, book.toml, SUMMARY.md) are authored
# separately and are NOT touched by this script. Safe to re-run.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)/src"

mode_line() {
  case "$1" in
    tutorial)    echo "**Mode: Tutorial** — a guided lesson; author-driven, guaranteed to work end to end. No detours into rationale." ;;
    howto)       echo "**Mode: How-to guide** — steps to a goal you already have; assumes basic competence. No teaching, no rationale." ;;
    reference)   echo "**Mode: Reference** — dry, complete, accurate; structured like the thing it describes." ;;
    explanation) echo "**Mode: Explanation** — discussion and rationale; the *why*, not the *how*." ;;
  esac
}

# stub <relpath> <mode> <status> <title> <oneliner> [generated]
stub() {
  local rel="$1" mode="$2" status="$3" title="$4" one="$5" gen="${6:-}"
  local path="$ROOT/$rel"
  mkdir -p "$(dirname "$path")"
  {
    printf '# %s\n\n' "$title"
    printf '<!-- This page is a Phase 0 stub. See ../../karn-documentation-plan.md -->\n\n'
    printf '> **Status:** %s\n>\n' "$status"
    printf '> %s\n\n' "$(mode_line "$mode")"
    if [ -n "$gen" ]; then
      printf '> ⚙️ **Generated page.** This page is intended to be generated from the compiler. Do not edit by hand once generation lands.\n\n'
    fi
    printf '%s\n\n' "$one"
    printf '_To be written._\n'
  } > "$path"
}

############################ Tutorials (Phase 1) ############################
P1="Planned — Phase 1 (the tutorial spine)."
stub tutorials/01-first-program.md tutorial "$P1" "1. Compile your first program" "Install \`karnc\`, write a trivial program, compile it to TypeScript, and read the output."
stub tutorials/02-http-service.md   tutorial "$P1" "2. Build a small HTTP service" "Use \`on http\`, return an \`HttpResult\`, wire up two or three endpoints, and run on Cloudflare Workers."
stub tutorials/03-modelling-data.md tutorial "$P1" "3. Model your data with types" "Work through opaque, record, and sum types via one worked domain example."
stub tutorials/04-refined-types.md  tutorial "$P1" "4. Make illegal states unrepresentable" "Introduce a refined type, validate input with \`.of\`, and handle the \`Result\`."
stub tutorials/05-stateful-agent.md tutorial "$P1" "5. Add a stateful agent" "Build an agent with zeroable \`state\` and observe fresh-state initialisation."
stub tutorials/06-testing.md        tutorial "$P1" "6. Test it" "Write \`test\` blocks, \`assert\`, mock a dependency with \`mocks\`, and fabricate values with \`Mock[T]\`."

############################ How-to (Phase 2) ############################
P2="Planned — Phase 2 (task coverage)."
stub how-to/index.md howto "$P2" "How-to guides" "Goal-titled recipes for readers who already know the basics. Each is independent — start wherever your task is."

stub how-to/refined-types/index.md            howto "$P2" "Refined types" "Tasks for defining refined types and admitting values into them."
stub how-to/refined-types/define-and-validate.md howto "$P2" "Define a refined type and validate untrusted input" "Define a refined type and run untrusted input through \`T.of\`, then handle the resulting \`Result\`."
stub how-to/refined-types/literal-admission.md howto "$P2" "Use a literal where a refined type is expected" "Rely on expected-type-directed admission for literals — and know when to reach for \`T.unsafe\` instead."

stub how-to/pattern-matching/index.md      howto "$P2" "Pattern matching" "Tasks for matching and narrowing values."
stub how-to/pattern-matching/match.md      howto "$P2" "Pattern-match with \`match\`" "Match on the variants of a sum type with \`match\`."
stub how-to/pattern-matching/narrow-with-is.md howto "$P2" "Narrow and bind with \`is\`" "Narrow and bind values with \`is\`, including complex \`is\`-receivers."

stub how-to/types/index.md                 howto "$P2" "Types & values" "Tasks for working with Karn's core types and values."
stub how-to/types/result-and-optionals.md  howto "$P2" "Work with \`Result\` and optional values" "Produce and consume \`Ok\`, \`Some\`, and \`None\`."
stub how-to/types/define-types.md          howto "$P2" "Define and consume sum, record, and opaque types" "Declare each of the three composite type kinds and consume them safely."
stub how-to/types/consumes.md              howto "$P2" "Consume a value with \`consumes … as\`" "Move-consume a value using \`consumes … as\`."

stub how-to/agents/index.md                howto "$P2" "Agents" "Tasks for building stateful agents."
stub how-to/agents/stateful-agent.md       howto "$P2" "Build a stateful agent and keep its state zeroable" "Build a stateful agent whose \`state\` satisfies the zeroability requirement."

stub how-to/http/index.md                  howto "$P2" "HTTP" "Tasks for handling HTTP requests."
stub how-to/http/handle-request.md         howto "$P2" "Handle an HTTP request and shape an \`HttpResult\`" "Handle an HTTP request and return a well-formed \`HttpResult\`."

stub how-to/testing/index.md               howto "$P2" "Testing" "Tasks for writing and running tests."
stub how-to/testing/write-tests.md         howto "$P2" "Write tests, mock collaborators, and pin a \`Mock[T]\`" "Write tests, mock collaborators, and pin a \`Mock[T]\` to a specific value."

stub how-to/projects/index.md              howto "$P2" "Projects" "Tasks for laying out and building Karn projects."
stub how-to/projects/layout.md             howto "$P2" "Lay out a project" "Set up \`karn.toml\`, the \`[paths] tests\` declaration, and the \`src/tests/*.test.karn\` location."
stub how-to/projects/cloudflare-workers.md howto "$P2" "Compile and target Cloudflare Workers" "Understand the two emission targets and build for Cloudflare Workers."

stub how-to/tooling/index.md               howto "$P2" "Editor & tooling" "Tasks for setting up the Karn toolchain."
stub how-to/tooling/format.md              howto "$P2" "Format your code with \`karn-fmt\`" "Format Karn source with \`karn-fmt\` (and \`karnc fmt\`)."
stub how-to/tooling/editor-support.md      howto "$P2" "Set up editor support" "Install the VS Code extension and syntax highlighting."

stub how-to/troubleshooting/index.md                          howto "$P2" "Troubleshooting" "One page per common diagnostic — paste an error code here to find the cause and fix."
stub how-to/troubleshooting/refine-literal-violates.md        howto "$P2" "\`karn.refine.literal_violates\`" "A literal didn't satisfy a refined type's predicate — cause and fix."
stub how-to/troubleshooting/agents-non-zeroable-state-field.md howto "$P2" "\`karn.agents.non_zeroable_state_field\`" "An agent state field can't be zero-initialised — cause and fix."
stub how-to/troubleshooting/mock-errors.md                    howto "$P2" "\`karn.mock.*\` errors" "\`needs_pin\`, \`outside_test\`, and \`unsupported_kind\` — Mock usage errors and fixes."

############################ Reference (Phase 3) ############################
P3="Planned — Phase 3 (reference & rationale)."
stub reference/index.md         reference "$P3" "Reference" "Consultable, complete, dry. Structured to mirror the language itself."
stub reference/grammar.md       reference "$P3" "Syntax & grammar" "The full grammar, derived from the grammar spec / \`tree-sitter-karn\`." gen
stub reference/keywords.md      reference "$P3" "Keywords" "Every keyword with one-line semantics." gen
stub reference/types.md         reference "$P3" "Type system" "Opaque, sum, record, and refined types; refinement predicates and literal-admission rules."
stub reference/refined-types.md reference "$P3" "Refined-type API" "\`.of\` (always \`Result\`), \`.unsafe\`, and the admission rules and where they apply."
stub reference/operators.md     reference "$P3" "Operators & built-ins" "Every operator and built-in, with types and semantics."
stub reference/agents.md        reference "$P3" "Agents" "Declaration, \`state\` rules, the zeroability requirement, fresh-state semantics, and lifecycle."
stub reference/http.md          reference "$P3" "HTTP" "\`on http\` handlers and the \`HttpResult\` shape."
stub reference/testing.md       reference "$P3" "Testing" "\`test\`, \`assert\` (both forms), \`mocks\`, \`Mock[T]\` semantics, test-context rules, and file layout."
stub reference/manifest.md      reference "$P3" "\`karn.toml\` manifest" "Every key, with \`[paths]\` and the legacy-vs-project mode behaviour."
stub reference/cli.md           reference "$P3" "CLI (\`karnc\`)" "Commands, flags, and exit codes." gen
stub reference/diagnostics.md   reference "Planned — diagnostic index lands in Phase 0; sourcing approach TBD (no central registry yet)." "Diagnostic / error-code index" "Every \`karn.*\` code with cause and fix." gen
stub reference/emission.md      reference "$P3" "Emission" "The TypeScript each construct emits, across both targets."
stub reference/changelog.md     reference "$P3" "Version compatibility & changelog" "What changed in each \`v0.X\` increment, and breaking-change notes."

############################ Explanation (Phase 3–4) ############################
P34="Planned — Phase 3–4 (rationale)."
stub explanation/index.md                       explanation "$P34" "Explanation" "The *why* behind Karn — safe to be opinionated, discursive, and to link out."
stub explanation/why-karn-exists.md             explanation "$P34" "Why Karn exists" "Architecture-first design, static typing, and compiling to typed TypeScript for Cloudflare Workers."
stub explanation/why-compile-to-typescript.md   explanation "$P34" "Why compile to TypeScript" "Interop and runtime fit — what that buys and what it costs."
stub explanation/type-system-philosophy.md      explanation "$P34" "The type-system philosophy" "Refinement, opacity, and errors-as-values; making illegal states unrepresentable."
stub explanation/refined-literal-admission.md   explanation "$P34" "The refined-literal admission model" "Why expected-type-directed admission, rather than overloading \`.of\` or adding a \`T.lit\` form."
stub explanation/the-agent-model.md             explanation "$P34" "The agent model" "What an agent is, and *why* state must be zeroable."
stub explanation/testing-philosophy.md          explanation "$P34" "The testing philosophy" "Why \`Mock[T]\` and test-context isolation exist; fabricated values vs real construction."
stub explanation/how-a-karn-program-is-shaped.md explanation "$P34" "How a Karn program is shaped" "The architecture-first mental model, end to end."
stub explanation/versioning-and-roadmap.md      explanation "$P34" "Versioning & roadmap" "The spec-first, incremental \`v0.X\` method; what's deferred to v1 and why \"deferred, not missing\" matters."
stub explanation/karn-compared-to-typescript.md explanation "$P34" "Karn compared to TypeScript" "Positioning against TypeScript and other typed languages — and when to reach for Karn."

############################ Reserved sections (Phase 4) ############################
P4="Reserved — Phase 4 (contributor & tooling docs). Stub only."
stub contributing/index.md reference "$P4" "Contributing to the compiler" "Architecture, fixtures, the bless workflow, and the \`tsc\` gate. Reserved for the contributor expansion."

stub tooling/index.md           reference "$P4" "Tooling" "Docs for \`karn-fmt\`, \`karn-lsp\`, \`tree-sitter-karn\`, and \`vscode-karn\`. Reserved for the tooling expansion."
stub tooling/karn-fmt.md        reference "$P4" "\`karn-fmt\`" "Formatting rules and configuration."
stub tooling/karn-lsp.md        reference "$P4" "\`karn-lsp\`" "The language server: features and setup."
stub tooling/tree-sitter-karn.md reference "$P4" "\`tree-sitter-karn\`" "The grammar used for highlighting and structural tooling."
stub tooling/vscode-karn.md     reference "$P4" "\`vscode-karn\`" "The VS Code extension."

echo "Generated stub pages under $ROOT"
