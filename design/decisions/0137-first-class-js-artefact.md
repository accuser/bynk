# 0137 — A first-class JavaScript artefact: `bynkc compile --emit js` by emit-then-strip

- **Status:** Accepted (in-browser track, slice 1; v0.108.1).
- **Provenance:** the second slice of the in-browser track. Slice 0 made the
  emitter strip-only (every emitted `.ts` is erasable by pure type-stripping —
  [ADR 0136](0136-strip-only-emission-invariant.md)); this slice turns that
  guarantee into a runnable JavaScript artefact, the form an in-browser eval needs
  (no `tsc` in the loop) and a minimal deploy artefact wants. Settles the track's
  open question on the JS production route in favour of a built-in strip pass.
- **Relation to prior records:** consumes the strip-only invariant
  ([ADR 0136](0136-strip-only-emission-invariant.md)) — the strip is total only
  because the emitter never produces a type-directed construct. Leaves
  TypeScript-first output the primary artefact ([ADR 0016](0016-no-portable-infrastructure.md)
  and the design notes' §19): TS still type-checks under `tsc --strict`; JS is
  additive and opt-in.

## Context

`bynkc` lowers Bynk to TypeScript. Two consumers cannot use TypeScript directly:
an in-browser eval that ships no transpiler, and a minimal deploy artefact that
wants plain JS. The production route was open: a built-in strip pass, the existing
external-`tsc` route, or shipping only a pre-stripped runtime. The external-`tsc`
route was rejected — it reintroduces the Node/`tsc` dependency the in-browser
direction exists to remove, and it cannot run in the browser at all (the wasm
toolchain slice needs JS produced in-process, in wasm, with no `tsc`).

Because the emitter is strip-only (ADR 0136), a JS artefact is *emit-then-strip*:
the same emitter output with type syntax erased and nothing else changed.

## Decision

**`bynkc compile --emit {ts,js}` (default `ts`). `--emit js` produces JavaScript by
type-stripping the emitted TypeScript in pure Rust — no `tsc`, no Node.**

- **D1 — The stripper is [oxc](https://github.com/oxc-project/oxc) in a dedicated
  `bynk-strip` crate.** oxc is a pure-Rust TS parser + type-erasing transform +
  codegen, and compiles to `wasm32` (so the wasm toolchain slice reuses this exact
  path in the browser). It lives in its **own crate**, not in `bynk-emit`, so the
  oxc dependency stays out of `bynk-emit` and therefore out of `bynk-ide`/`bynk-lsp`
  — the language server needs neither stripping nor JS output. A hand-rolled
  stripper was rejected: the `:` / `<…>` / `as` / `type`-specifier ambiguities in
  the emitted surface require a real parser, not a lexical pass.

- **D2 — Pure type-stripping, not usage-based elision.** The transform is
  configured `only_remove_type_imports`, so **every value import is preserved**
  (even if unused) and only `import type` / `type` specifiers and type syntax are
  erased. This matches Node's `stripTypeScriptTypes` strip-only mode (the slice-0
  strip oracle): stripping is a syntactic erase, never a semantics-aware rewrite.
  `enum`/`namespace` are handled gracefully rather than panicking (a defensive
  `with_enum_eval`), though the strip-only invariant means they never appear.

- **D3 — Stripping is a post-emit step in `bynkc`, not a mode of the emitter.**
  `bynk-emit` still emits only TypeScript; `bynkc::strip_project_to_js` rewrites a
  compiled `ProjectOutput` to JS: every `.ts` module is stripped and renamed to
  `.js`, the `tsconfig.json` is dropped (a TS-compiler config with no role for a JS
  artefact), source maps and the debug sidecar are dropped (they map into the `.ts`
  the JS replaces), and any other file (e.g. `wrangler.toml`) passes through.
  Import specifiers are already `.js` (the default `ImportExt`), so the renamed
  tree resolves unchanged. Keeping the emitter TS-only preserves its single
  responsibility and keeps oxc downstream of it.

## Consequences

- `bynkc compile --emit js` yields a runnable JavaScript tree with no `tsc`/Node
  dependency; every emitted `.js` parses under `node --check` (the verification
  test) — which is also a residue check, since a surviving annotation would fail
  it. `--emit js` is target-agnostic: it also strips a `--target workers` build.
- TypeScript stays the default and primary artefact; the JS path is additive. The
  emitter, its fixtures, and the `tsc --strict` gate are untouched.
- The wasm toolchain slice reuses `bynk-strip` to produce JS in the browser — the
  same mechanism, one strip implementation across CLI and playground.
- `bynk-strip` is a new published crate (its first publish goes through the
  release-bootstrap path before trusted publishing). It pulls oxc — a substantial
  dependency, isolated to this crate and `bynkc`, so `bynk-emit`/`bynk-ide`/`bynk-lsp`
  remain lean. oxc requires a recent toolchain; if the MSRV CI leg (built on the
  declared floor) trips on an oxc API, the floor rises to match (the sanctioned
  response), rather than dropping oxc.
