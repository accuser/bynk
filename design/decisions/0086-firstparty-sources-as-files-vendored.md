# 0086 — First-party sources are authored as files (embedded via `include_str!`), vendored not published

- **Status:** Accepted (v0.48)
- **Spec:** none (internal refactor; no language surface, byte-identical emitted output)
- **Supersedes the literal-blob arrangement; sits alongside the platform decisions 0017/0026.**

## Context

The first-party Bynk surface, the platform adapters, the Bynk-written
collection/string commons, the per-platform TypeScript bindings, and the emitted
runtime are all real Bynk and TypeScript programs — but they lived as Rust
`r#"…"#` string literals (`firstparty.rs`, and `RUNTIME_TS` in `emitter.rs`).
As literals they sat outside every tool that could check them: the `.bynk`
sources never reached the lexer/parser/`bynk-fmt`/`bynk-lsp` directly, and the
`.ts` bindings + runtime escaped `tsc --strict` except transitively when a
fixture happened to emit them. The drift between the live runtime and a stale
39-line `bynkc/runtime/runtime.ts` stub was the concrete failure mode — and v0.47
made it sharper by putting the security-critical Bearer JWT verifier into the
runtime literal.

## Decision

First-party sources are **authored as real files with their real extensions**,
under a package-shaped `bynkc/src/firstparty/` tree, and **embedded at compile
time via `include_str!`** — the same `&'static str` values, no call-site change,
one self-contained binary. The single source of truth replaces the literal +
stale stub. This makes them visible to the compiler's own pipeline, `bynk-fmt`,
`bynk-lsp`, `tsc`, editors, and (once they are files) SAST and standalone tests.

They are **vendored, not published.** The bindings and runtime are part of the
compiler's **emit ABI** — coupled to emit shapes (`Result`/`Option` tag layout,
`JsonError`, `Uuid.of`, `FetchError`). Vendoring (emitting them into each output
project) makes version skew impossible by construction: the compiler that emits
your code emits the runtime it talks to. Publishing them as independently-versioned
`@bynk/*` packages would introduce a skew axis the compiler must then police,
invert the import graph, and add a registry dependency to currently-hermetic
builds — not worth it pre-1.0. The package-shaped layout is the cheap hedge that
keeps a future publish a lift-and-shift; **publishing is deferred to a future ADR,
gated on runtime-ABI stability (≈1.0).**

Embedding is `include_str!`, not a `build.rs` codegen step (zero dependency;
a missing path is already a hard compile error).

## Consequences

The first-party stdlib/runtime/bindings gain standing checks: each `.bynk` source
must parse and be `bynk-fmt`-clean, and the embedded `runtime.ts` passes
`tsc --strict` standalone — closing the "only checked when something uses it"
gap. Reformatting a `.bynk` source changes no emitted TypeScript (formatting is
trivia), so this is independent of the byte-identical emitted-output guarantee the
golden + `tsc_verify` + `runtime_helpers` suites pin. Crucially, it **unblocks
v0.49**: the now-real `runtime.ts` is a real import target for behavioral
auth-bypass tests and is visible to CodeQL. The `.ts` bindings remain
tsc-covered transitively; their standalone scaffold is a follow-up.
