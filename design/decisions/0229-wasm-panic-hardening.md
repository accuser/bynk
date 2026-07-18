# 0229 — Bound the blast radius of an internal compiler panic in the playground wasm

- **Status:** Accepted (v0.206.1)

**Context.** `bynk-wasm` runs the full lexer/parser/checker/emitter pipeline on
arbitrary playground input. Every non-panic failure is already returned as a
structured diagnostic, but a panic was not: the crate pulled no panic hook and
wrapped no entry point, so a reachable-in-principle `panic!` / index-out-of-bounds
/ `unreachable!` in the pipeline aborted as `RuntimeError: unreachable` in the
browser with no diagnostic, and left the wasm instance in a state that misbehaved
on subsequent calls until reload (#717). Fixing each underlying panic site (the
parse depth limit, non-char-boundary slicing) is tracked as sibling work; this
decision is about the boundary itself.

**Decision.** Two independent hardening layers:

- Install `console_error_panic_hook` (idempotent `set_once`) at the top of the
  `bynk_analyze` / `bynk_compile` wasm entry points. A panic then routes to
  `console.error` with a readable message and location instead of an opaque trap.
- Wrap the shared `compile` / `analyze` bodies in `catch_unwind`, converting a
  caught panic to a `bynk.wasm.panic` error diagnostic (`catch_panic`).

The catch boundary was measured, not assumed: on stock `wasm32-unknown-unknown` a
panic still traps — the target lowers unwinding to `unreachable`, so `catch_unwind`
does **not** recover and the panic-hook message is the only mitigation there. The
same `catch_unwind` genuinely unwinds on the native `rlib` path (the tests and any
host embedding), so a panic no longer propagates past the boundary, and the
wrapper becomes effective on wasm for free if the playground build ever adopts
wasm exception handling. Making it catch on wasm today would require nightly
`build-std` + the `exception-handling` target feature + a wasm-EH-capable
`wasm-opt`/runtime — disproportionate for a P2.

**Consequences.** A playground panic is now legible in the console rather than a
bare `RuntimeError`, and the native/embedding path is fully hardened. The wasm
instance can still be poisoned by a trap until reload; that is bounded, not
eliminated, and the durable fix is removing the underlying panic sites. The new
`console_error_panic_hook` dependency is wasm-only (native builds never pull it).
