# 0008 — Cross-context capabilities wire by local instantiation, not remote routing

- **Status:** Accepted (v0.15)
- **Spec:** §7.3.6, §5.8

## Context
When context A depends on a capability context B provides, B's provider must
run somewhere: instantiated locally in A's composition, or routed to B's Worker
over the Service-Binding protocol.

## Decision
**Local instantiation (A1).** The consumer's composition instantiates the
provider class from the providing unit; the capability call is an in-process
method call, never a wire crossing. Remote routing remains the natural shape
for *stateful* shared capabilities, deferred with sagas/coordination.

## Consequences
Stateless platform capabilities get per-Worker instances (matching the runtime
model); v0.12's wiring is reused almost verbatim. Provider code is bundled into
each consumer — accepted, capabilities are shared contracts. Adapters (0010)
inherited this in-process model wholesale.
