# 0013 — Bindings are named by an explicit clause; npm deps are declared there

- **Status:** Accepted (v0.17)
- **Spec:** §4.1.19, §8.6

## Context
Compose must import provider implementation symbols from some TypeScript
module; the rule had to be explicit and greppable rather than filename magic.
Third-party adapters also need a declared, reviewable npm-dependency surface.

## Decision
`binding "<module>" requires { "pkg": "range" }` — the path resolved relative
to the adapter's source file; the module copied verbatim into the output;
`requires` folded into the generated `package.json`, with ranges **pinned** (a
range must name a version digit; `*`/`latest`/digit-free rejected). First-party
bindings are **emitted into the project** (inspectable), not shipped as a
package.

## Consequences
Rename-safe, greppable resolution; `tsc --strict` checks the `implements`
contract. The full third-party trust policy stays open (0020).
