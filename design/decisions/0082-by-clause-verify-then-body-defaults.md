# 0082 — The `by` clause; two-phase verify-then-body; per-protocol default actors

- **Status:** Accepted (v0.45)
- **Spec:** `syntactic-grammar.md` (`by_clause`), `static-semantics.md` (`missing_by_on_http`, `scheme_not_admissible`, default-actor resolution), `runtime-library.md` (prelude actors)
- **Realises:** the actors track, question Q5.

## Context

How does a handler consume an actor, where does the binding sit, and what runs
before the body? And must every handler spell out its actor, or can the common
internal case be inherited?

## Decision

A handler consumes an actor on a **`by <name>: <Actor>` clause**, positioned on
v0.44's header-relative handler forms **after the protocol config and before the
parameters** (`on schedule("…") by s: Scheduler () -> …`). The verified actor
binds to `<name>`; its identity is `<name>.identity` (0081).

Verification is **two-phase and fail-closed**: it must succeed before the body
runs, with the payload already parsed — no verified identity ⇒ no body. This is
structural (the `by` clause is the boundary's authority-minting entry point), not
a runtime guard the body could skip.

**Per-protocol default actors**: with the protocol on the `from` header (0077), a
handler that omits `by` inherits the protocol's default — cron → `Scheduler`,
queue → `Producer`, `on call` → `Caller`, all `Internal`. Inheritance is
**silent** (demanding `by s: Scheduler` on every cron handler is ceremony).
**HTTP has no safe default, so `by` is required** (`bynk.actor.missing_by_on_http`)
— a public route writes `by v: Visitor` explicitly; there is no implicit
anonymous surface. A scheme must be admissible on its protocol
(`bynk.actor.scheme_not_admissible`): HTTP admits `None`, the internal protocols
admit `Internal`.

The seam is a sealed **scheme descriptor** mirroring the protocol descriptor
(0079): `None` emits nothing (always admits); `Internal` reuses the channel-trust
assertion already implicit in service-binding / platform dispatch — so Foundations
adds no target topology beyond the gate.

## Consequences

Authority is visible in the handler signature, never ambient; the common internal
case stays ceremony-free while the one unsafe surface (HTTP) stays loud. The
clause and defaults are written against v0.44's handler forms, so they move with
that surface. Adding Bearer/Signature is new surface against the scheme
descriptor, not a re-architecture.
