# 0310 — The emit ABI is a small published surface; the codegen underneath it is not

- **Status:** Accepted (v0.246.1)

**Context.** [[0086]] made the first-party sources real files under `bynk-check/src/firstparty/`,
embedded by `include_str!`, and enumerated what the emit ABI actually is: "The bindings and runtime
are part of the compiler's **emit ABI** — coupled to emit shapes (`Result`/`Option` tag layout,
`JsonError`, `Uuid.of`, `FetchError`)." It deferred publishing them: "gated on runtime-ABI stability
(≈1.0)."

`bynk-1.0-definition.md` states that stability does **not** freeze "**The emitted TypeScript.** The
compile target is an implementation detail, not part of the frozen contract; the codegen may improve
within a 1.x release as long as documented behaviour holds."

**An earlier revision of this record read those two as a contradiction** and concluded that the
bindings must stay vendored, with exact-version lockstep recorded as the eventual shape. That
reading was wrong in three ways, and the settling review found all three.

**It conflated two surfaces of wildly different size.** [[0086]]'s enumeration is *four things*. The
codegen — how a `match` lowers, how a handler is emitted, the Durable Object class shape,
`loadState`/`commitState` — is the entire back end. Publishing the four does not freeze the rest, and
only the conflation made it look as though it did.

**It borrowed one layer from a three-layer precedent.** It cited [[0200]] as its pattern while
adopting only the runtime check. [[0200]]'s own title is "one canonical normal form, **fail-closed at
runtime, refused at deploy**", and its body adds a standing guard
(`bynkc/tests/contract_hash.rs`) asserting over the whole fixture corpus that every stamped hash
equals its callee's constant. A load-time check is the layer that fires last — at the customer.

**And exact-version lockstep is unworkable here specifically.** Bynk assigns a version per merge
([[0206]]). A binding pinned to an exact compiler version is stale within a day. That does not defer
the collaboration story; it forecloses it.

**Decision.** Four parts.

**D1 — The emit ABI is exactly [[0086]]'s enumeration.** `Result`/`Option` tag layout, `JsonError`,
`Uuid.of`, `FetchError`. Anything not on that list is codegen. The list is the contract, and it is
short on purpose.

**D2 — It is published, with its own semver, versioned independently of the compiler.** Adding a
shape is a minor version of the ABI package; changing an existing shape is a major one. Neither is a
Bynk *language* event, because the ABI is not the language.

**D3 — The codegen is not part of it**, and `bynk-1.0-definition.md`'s freedom to improve the
emitted TypeScript within 1.x is retained in full.

**D4 — Skew is caught at three layers, per [[0200]], not one.** A standing build-time guard
asserting the vendored first-party bindings reference only the enumerated surface; a deploy-time
refusal; and a fail-closed check at load. The build-time guard is the one that matters most, because
it is the only one that fires before anything ships.

**Consequences.** The enumeration becomes load-bearing and must be maintained. The defect this ADR
has to guard against is a *fifth* shape leaking from the codegen into a binding without being added
to the list — at which point the codegen is frozen by accident, which is exactly what the earlier
revision feared and mis-diagnosed the cause of. D4's build-time guard exists for that and nothing
else.

[[0086]]'s "version skew impossible by construction" property is given up deliberately, and replaced
by three layers of detection. That is a real trade: vendoring made skew unrepresentable, and
publishing makes it representable but legible. The compensation is that the collaboration story for
capabilities stops being deferred — a capability adapter authored outside this repository has
something to import.

**None of the implementation is this track's work.** This record settles the posture; packaging the
ABI, wiring the three guards, and the `@bynk/*` publication mechanics are packaging-track work and
appear in `design/tracks/compiler-architecture.md` §7 as a forward reference. The tier taxonomy
separating a substrate-free capability from one requiring a runtime ABI is in
`design/bynk-greenfield-compiler.md` Part 14; the extension point it names (E7, transaction
participation) is the first thing that would exercise a published ABI, and it remains unscheduled.

---
