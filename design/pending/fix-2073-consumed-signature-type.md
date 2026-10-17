---
level: minor
probes: diagnostic_coverage, emit_diagnostics, ts_writes
changelog: A call to a consumed context's service whose signature names a type that context consumes (`isDomestic(o: Order)`, `Order` from a third context) now checks and runs for every caller, `system` suites included. A caller must consume every context that a called service's signature types reach (`bynk.consumes.signature_type_unconsumed`); this also refuses a call such as `shop.order.place`, returning `shop.order`'s own `Order` with a `shop.base` field, from a caller that does not consume `shop.base`. That call compiled on `bundle` but failed `tsc` on `workers`. The `X-Bynk-Contract` hash of every service whose signature names a consumed context's type now describes that type's shape, so it moves for services that already worked; redeploy both Workers of such a pair together, or the call answers `ContractMismatch`
---

## ADR: consumed-signature-types
title: A service's signature may name a type its context consumes, resolved in the declaring context
summary: Callers resolve, hash and codec a borrowed signature type through the context that declares it

**Context.** A service's signature can name a type that its context consumes
from another context: `shop.ledger`'s `on call(o: Order)`, with `Order` from
`shop.order`. A caller of `shop.ledger` saw `shop.ledger`'s types and its
`uses` commons' types, but not the types `shop.ledger` itself consumes. So
`Order` resolved to `()`, and every caller was refused with
`bynk.boundary.structural_mismatch`, `system` suites included (#2073). The
contract hash (`X-Bynk-Contract`, #643) was built from the
same table on both sides, so it hashed such a type as unresolved.

**Decision.**

- **One table for a context's signatures.** `symbols::contract_types_for`
  builds it from the context's own types, its `uses` commons' types and the
  types of the contexts it consumes. The context's own types win a name
  clash. It records the declaring context of each borrowed type. A caller's
  view of the consumed context and both sides of the contract hash use this
  table.
- **A borrowed type's body resolves in its declaring context.** `shop.order`'s
  `Order { region: Region }` names `shop.order`'s `Region`, even when
  `shop.ledger` declares its own `Region`. Where the two would differ, the
  table holds the declaring context's declaration under a qualified key
  (`shop_order__Region`). The emitter names that key in TypeScript by its
  declaration's own name, through its context (`shop_order.Region`). A name
  that two consumed contexts both declare is added bare for neither. A
  signature cannot name it bare anyway, because an exported clash is already
  `bynk.consumes.name_conflict`.
- **A commons type behind a borrowed type stays the commons'.** Take
  `shop.order`'s `Order { region: Region }`, with `Region` from a commons that
  `shop.order` uses. `Region` is added to the table under its own name, with
  no owning context, and its codecs come from the commons, as for any commons
  type. It can never land under a qualified key, because a consuming context
  that binds `Region` to something else is already
  `bynk.uses.name_conflict` (#1814).
- **The caller must consume every context the signature's types reach.** A
  value of such a type crosses the call as its declaring context's type, so
  the caller names it, and builds its codecs, through that context. A caller
  that does not consume it is refused with
  `bynk.consumes.signature_type_unconsumed`, which names the declaration
  (`Base`) and its context. The rule covers every type the signature reaches,
  not only a borrowed one. `shop.order.place`, returning `shop.order`'s own
  `Order { b: Base }`, is refused for a caller that does not consume
  `shop.base`. That program passed the checker before, compiled on `bundle`,
  and failed `tsc` on `workers` (the caller had no codec for `Base`). It is
  now refused on both.
- **The contract hash moves.** A service whose signature names a consumed
  context's type now hashes that type's shape. Before, it hashed the type as
  unresolved. Services that already worked, such as one returning a consumed
  `Receipt`, get a new `X-Bynk-Contract`. A deployed pair built across this
  change answers `ContractMismatch` until both Workers are redeployed.

**Consequences.** This is a `minor` increment rather than a `patch`. The
checker accepts a new class of program, and it adds a refusal. The contract
hash is a published, deploy-visible surface (ADR 0483), and it moves for
existing services. A deploy that pushes only one Worker of such a pair is
refused at the call until the other side is redeployed. The IDE's
wire-contract peek reads the same table, so its hash agrees with the emitted
constant. It reports a borrowed type as the unit's own.
