---
level: minor
changelog: A bundle-mode `on call … by c: Caller` handler reads a live `CallerId` — its emitted `makeSurface` deploy surface threads the calling context's name into `deps.identity`, where it previously emitted `deps` without the field and broke `tsc`
---

## ADR: bundle-caller-surface-identity
title: The bundle cross-context surface threads the caller name into a `by c: Caller` handler's deps
summary: How `makeSurface` and the compose root supply the `CallerId` a bundle-mode Caller handler reads, and why the value differs per call site

**Context.** A cross-context `on call … by c: Caller` handler reads a live
`CallerId` — the calling context's qualified name — threaded through
`deps.identity` (ADR 0092). In Workers mode the caller name rides an
`X-Bynk-Caller` header the callee reads at its entry, and the compose wrapper
threads it into deps. In **bundle** mode the same handler emits a `call` method
whose `deps` is typed `{ …; identity: string }`, but the emitted cross-context
deploy surface — `makeSurface` (`bynk-emit/src/emitter/emit.rs`) — forwarded the
context's `<Ctx>Deps`, which carries no `identity`. The result type-checked at
`bynkc check` (a Bynk-level pass) but failed `tsc`: `makeSurface` called
`svc.call(args, deps)` against a `deps` missing `identity` (TS2345). Because
`bynkc test` runs `tsc` over the whole project, a single named binder took the
entire test run down, pointing at generated code the author never wrote (#655).
The value half of the feature was simply never wired on the bundle target — a
bundle `by c: Caller` handler could not compile at all.

**Decision.** **The bundle cross-context surface threads the caller name into a
Caller-binding handler's deps, supplied at the wiring seam — the compose root —
exactly as Workers supplies it at the entry seam.**

- `makeSurface` gains a second `__caller: string` parameter **only** when the
  context declares a Caller-binding `on call` handler, and threads
  `{ ...deps, identity: __caller }` into that handler's `svc.call`. Every other
  handler forwards `deps` verbatim, so a caller-free surface is byte-unchanged.
- The compose root (`composeApp`, `bynk-emit/src/project.rs`) supplies the name.
  A **consumer** edge builds a per-consumer surface — `B.makeSurface(BDeps,
  "<consumer>")` — so the callee reads the *consuming* context's qualified name,
  matching Workers' `X-Bynk-Caller`. The shared per-provider surface instance is
  kept only where no consumer needs a distinct caller.
- The **top-level** entry the compose root returns addresses a context directly,
  with no calling context; it passes the context's *own* qualified name as a
  stable, non-empty `CallerId`. A bundle is a single trust domain (ADR 0092), so
  a self-attributed direct call mints nothing and crosses no boundary.

**A deliberate cross-target divergence.** The top-level seam is where the
`Caller` value stops being identical across targets, and this ADR owns that. In
Workers a context is reached only through its internal `/_bynk/call/` door,
which **fail-closes** an unattributed call (no `X-Bynk-Caller` → the internal-
channel analogue of 401, ADR 0092). A bundle's top-level entry is a *programmatic*
surface, not that internal door, and it has no calling context to fail-close on;
self-attribution is the only stable, non-empty value available there. So a
`by c: Caller` handler invoked *directly* at the bundle's top-level reads its own
name where the same handler in Workers would refuse the call. This affects only
the unattributed top-level path — a genuine cross-context call (consumer → provider)
reads the consumer's name identically on both targets, which is the path real
programs take.

The caller-binding predicate is a single `pub(crate) any_service_binds_caller`
(in `emit.rs`) that both `emit_make_surface` and the compose root's
`context_binds_caller` (in `project.rs`) call — so the two seams cannot disagree
on which providers take the extra `__caller` argument, even under a future
refactor of `caller_binder_for`'s internal `HandlerKind::Call` guard.

**Consequences.** A bundle-mode `by c: Caller` handler now compiles and reads the
real caller: `bynkc test` over such a service passes, and a cross-context call in
a bundle reads the consuming context's name. This is a language increment: the
value the handler observes is now defined on the bundle target where it
previously failed to compile. The Bearer/Oidc/actor-sum seams are unaffected —
their sealed identities are minted only at a verification seam (ADR 0081) and are
not addressable from the bundle surface. Two coverage additions close the gap
that let #655 ship: a positive fixture drives a `by c: Caller` handler through a
`suite` on the bundle target (pinning the `makeSurface`/`compose.ts` output and
the TS2345 that broke, via `tsc_verify`), and a bundle-mode behavioural test
(`bynkc/tests/cross_context_caller.rs`, the twin of the existing Workers one)
composes the app and asserts each consumer reads its own name (`app.a`/`app.d`)
and the top-level entry self-attributes (`app.b`).
