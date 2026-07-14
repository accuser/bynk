# 0192 — Multi-context local dev: every context served, Service Bindings wired; supersedes ADR 0096 D3

- **Status:** Accepted (v0.167)
- **Realises:** the multi-context local dev increment (closes #552, resolves the
  Bynk Language Design Review (2026-07-05) Platform #5 finding, §5.5/§8).
- **Relates / supersedes:** [[0096]] (`bynk dev` — this ADR **supersedes its D3**,
  the select-or-default rule that refused a multi-context project; D1, D2, D4 and
  D5 stand), [[0084]] (the `doctor` capability/exit contract `dev` pre-flights),
  [[0104]] (the `--inspect` debug launch model, whose port becomes an
  allocation), [[0017]] (platform lock per deployment unit).

## Context

Bounded contexts talking over Service Bindings are the flagship architectural
feature, and the emitter has generated the wiring correctly since v0.57: a
`consumes` becomes a `[[services]]` entry keyed by binding name, the call lowers
to `callService(deps.env.COMMERCE_PAYMENT, …)` over an internal `fetch`. But
nothing ever ran two of those Workers **at once**. `bynk dev` served exactly one
(ADR 0096 D3: one context → served; `--context` → chooses; several → fail and
list them), so the moment a project grew a second context the flagship feature
became unrunnable locally. D3 named this a limitation rather than half-doing it,
and logged multi-worker dev as next intent — this is that increment.

The consequence was sharper than "a missing convenience": a cross-context call
had **never been exercised at runtime anywhere in the repo**. The `[[services]]`
output was covered by golden fixtures only, and the one live Workers test
(`workers_runtime_smoke.rs`) serves the single-context hello-world. The wiring
was asserted as text, never as behaviour.

The decisions below are the defining calls — what runs the workers, what `dev`
does by default, who owns the ports, and what a partial session means.

## Decision

**(D1) One `wrangler dev` per context, wired by the dev registry — not a
first-party `workerd` server.** The recommendation attached to #552 said
"workerd with wired Service Bindings", and ADR 0096 anticipated a v1 first-party
`workerd` server replacing the wrangler hand-off. Neither is needed to close the
gap. Wrangler runs a **dev registry**: independent `wrangler dev` processes
discover each other and wire their `[[services]]` bindings between themselves.
Verified against wrangler 4.103.0 on the two-context fixture — the binding
reports `local [connected]`, `placeOrder` reaches `commerce.payment` over the
binding, and payment's `Declined` maps through orders' anti-corruption layer to
`PaymentDeclined`. So the increment is **orchestration only: the emitter is
untouched.** A first-party `workerd` server remains available as the substitution
ADR 0096's encapsulated serve step was designed to permit — it is now an
optimisation, not the price of the feature.

Two traps are recorded because both look like the answer and are not. Repeated
`-c` flags (`wrangler dev -c a/wrangler.toml -c b/wrangler.toml`) do **not** start
an auxiliary worker: wrangler silently serves the first config alone and the
binding sits at `[not connected]`. And start order does **not** need staging —
a binding begins `[not connected]` and converges once its callee is up, so the
driver spawns all workers at once and lets the registry reconcile.

**(D2) No `--context` serves *every* context; `--context` is repeatable and
narrows.** This supersedes D3. The ambiguity error was the feature being
withheld, not a project being wrong: a cross-context call only resolves when its
callee is up too, so "several contexts" is the *expected* shape of the flagship
architecture, not a fork the author must resolve. `--context` becomes repeatable
for the cases where fewer processes are wanted, resolving the dotted or
dasherised name exactly as before. `SelectError::Ambiguous` therefore leaves
`dev` entirely; it survives only for `deploy`, which still ships one Worker at a
time, and its message drops the `--context` remedy it can no longer offer.

**(D3) Port allocation is the driver's own concept.** `wrangler dev` binds one
port per process, so N contexts need N ports — allocation is no longer a wrangler
flag to forward but a thing the driver must decide, which is exactly ADR 0096
D5's test for what the driver may curate. Context *i* gets `--base-port` + *i*
over the deterministic worker order (default 8787, wrangler's own), and
`--inspect-port` bases the inspector allocation the same way ([[0104]]).

The pre-#552 contract is preserved by one exception: a **lone** worker with no
explicit `--base-port` gets no injected `--port` at all, so it lands on
wrangler's default and `-- --port N` keeps working. This is load-bearing rather
than cosmetic — wrangler rejects a repeated `--port` with a usage dump ("expects
a single value, but received multiple") instead of taking the last one, so
injecting unconditionally would have broken that passthrough. Where the driver
*does* inject, the same flag arriving through `--` is caught with a message
naming the driver flag that owns it. (ADR 0096's claim that an explicit
`-- --inspector-port N` "still wins" was, for the same reason, never true; it is
withdrawn here.)

**(D4) Any worker exiting ends the session, and teardown is SIGTERM.** A
survivor's bindings point at a context that is gone, so a half-served project
fails in a way that reads as a code bug — `dev` stops the rest and propagates the
first exit code. The signal is **SIGTERM, not `Child::kill`'s SIGKILL**: wrangler
traps SIGTERM and tears down the `node` and `workerd` processes it spawned,
whereas SIGKILL is untrappable and strands an orphaned `workerd serve` still
holding the port — verified, and a stranded one makes the *next* `bynk dev` fail
on a port clash. std exposes no SIGTERM, so teardown goes through POSIX
`kill(1)`, escalating to SIGKILL after a grace period so a wedged shutdown cannot
hang the session. Ctrl-C is unchanged and needs no forwarding: the driver and
every wrangler share the terminal's foreground process group (ADR 0096 §Exit),
which is verified to still hold for N children.

## Consequences

The flagship architecture runs locally: `bynk dev` in a multi-context project
serves every context, prints which one answers on which URL, and a cross-context
call resolves through a live Service Binding. The watch loop (ADR 0096 D2's
follow-up, shipped in #524) composes with it — editing a *callee*'s `.bynk`
source rebuilds and hot-reloads that worker, and the change is observed through
the binding by its caller, with the wiring intact across the rebuild.

The cross-context Service Binding now has runtime coverage for the first time,
rather than golden-file coverage of the generated text. That the emitter needed
no change is the finding, not a coincidence: the wiring was right all along and
only the orchestration withheld it.

Two things are deliberately **not** in scope, and neither is silently half-done.
`deploy` still ships one Worker at a time — a multi-context project cannot deploy
(it fails as ambiguous, now with an honest message), which is `deploy`'s own
problem to solve and is tracked separately. And the first-party `workerd` server
stays a named follow-up (D1), now motivated by what it would *add* — a single
front door, unified logs — rather than by the cross-context calls it is no longer
needed to deliver.
