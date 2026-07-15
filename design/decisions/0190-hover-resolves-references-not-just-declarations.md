# 0190 — Hover resolves *references*, not just declarations; a structural resolution outranks a name match

- **Status:** Accepted (v0.165.0; 2026-07-14)
- **Provenance:** the v0.165.0 hover-references increment — an LSP-only change closing #611. Inside an `agent` handler body, hover worked on **declarations** but missed three kinds of **reference**: a `store`-field use (`lastSeq + 1`), a record-construction field label (`Stored { seq: …, title: … }`), and a store method call (`items.put(…)`). Two failed silently; the third was worse — `title:` rendered the enclosing handler param `add(title: Title)`, a confidently *wrong* answer.
- **Realises:** hover over the body of an agent handler — the code a reader actually spends their time in, as opposed to the four lines that declare its state — and an enumerable store-operation registry the editor reads, pinned to the checker's dispatch.
- **Relates:** ADR 0161 (the `key`/`store` declaration hover this extends from declaration to reference); ADR 0063 (the enumerable kernel-method registry this mirrors for storage operations); ADR 0069 (the `Field`/`Method` index kinds whose `Type.field` keys hover now renders); ADR 0110/0113/0121/0125 (the storage-map/cache/log/cell operations the registry enumerates).

## Context

Hover had grown one path per *declaration* kind — top-level symbols
(`describe_symbol`), locals and params (`describe_local_at`), `self`, agent
`key`/`store` fields (ADR 0161), handler annotations. Every hover test asserted at
a declaration offset. The reference side was never systematically covered, and
three gaps had accumulated behind that blind spot; #611 is them made visible.

The three had *different* causes, which is the interesting part:

- **A store-field reference resolved nowhere.** State fields are absent from the
  project index (there is no `SymbolKind` for them) and are not `let`/param
  locals, so every path missed a use. ADR 0161's path was declaration-position-only
  — it matched the `key`/`store` keyword span and the field-name declaration span.
- **A record field label resolved, then failed to render.** The checker *does*
  record construction labels as `Field` refs keyed `"Stored.title"` (ADR 0069), so
  the index resolved the offset correctly — but `describe_item` only matched
  top-level declaration names, had no `Field` arm, and returned `None`. Hover then
  **fell through** to the locals path, which matches by *name in scope*, and bound
  `title:` to the same-named handler param.
- **A store operation had no path at all.** `qualified_callee_at` bails on a
  lowercase receiver, so `signature_help::resolve_label` was never reached; and the
  storage entry ops (`put`/`get`/`update`/…) are checked in `match` arms — never
  indexed, and not enumerable by anything.

The second is the one worth generalising from. A resolution that is *structurally
correct* (the index resolved this exact span to this exact symbol) was discarded
because the renderer had no arm for it, and a *guess* (a local with the same
spelling) answered instead. The fall-through was silent: no test, no diagnostic,
just a plausible wrong hover.

## Decisions

**D1 — Where a resolved index hit has a renderer, it is rendered; the fix for a
missing arm is the arm, not a wider fall-through.**
`describe_symbol`/`describe_item` gain a `Field` arm for the compound
`"Type.field"` keys ADR 0069 already produces, so a resolved label renders the
record's field (its declared type and any `where` refinement, attributed to its
owner) rather than dropping to the locals path. Top-level names carry no `.`, so a
compound key can only match the new arm — the two namespaces cannot collide.

The guiding principle is that **a structural resolution outranks a name match**.
This increment does not *enforce* it: the index rung still guards on the renderer
returning `Some`, so a resolved key with no arm — `Method`, `CapabilityOp`,
`Actor` — still falls through. Making the rung return `None` on a
resolved-but-unrendered key would enforce it, and was measured rather than
assumed: for `Method` the fall-through reaches `qualified_callee_at` →
`resolve_label`, which returns `None` (a lowercase receiver at a use site; an
unresolvable label at the declaration), and `Actor` reaches nothing either — so
today those kinds already hover as *nothing*, and enforcing D1 would buy no
user-visible change while risking the `CapabilityOp` case, where a project
capability's `Upper.op` receiver *can* reach `resolve_label`. The rule is stated as
the direction of travel; the arms are what will make it true, one kind at a time.

**D2 — A reference to agent state renders exactly what its declaration renders.**
Hovering `lastSeq` in `let next = lastSeq + 1` and hovering `store lastSeq:
Cell[Int]` answer the same question — "what is this field?" — so they return the
same content, from the same builder. This extends ADR 0161 D2's reasoning (the
keyword and the field name it introduces render alike) one step further out, to
the uses.

**D3 — The reference scope is where state is *referenceable*, not the agent's
span.** The reference pass matches by name (state fields are not in the index, so
there is no span to match against), which makes over-reach the live risk. It is
scoped to handler bodies and invariant/transition predicates — the positions where
a bare name actually resolves to state. Deliberately excluded is the declaration
region itself, where a same-named identifier means something else: `@indexed(by:
id)` names a field of the **stored value**, not the agent's `key id`. ADR 0161's
test already pinned that `id` must not hover as the key field; scoping to the
reference positions is what keeps it pinned.

**D4 — Name-based resolution defers to the checker's own dispatch rule.** The
checker dispatches a store op on a bare-ident receiver **not in the value scope**;
a local of that name shadows the field. Both name-based paths honour that rule
rather than approximating it: the state-reference pass runs after the locals path
(so a shadowing local wins), and the store-op path is handed the locals table and
declines when the receiver is shadowed. Where the editor must guess by name, it
guesses the way the compiler binds.

**D5 — Storage operations get an enumerable registry, pinned to dispatch —
`store_ops`, beside `kernel_methods`.** The op names live in the checker's `match`
arms: authoritative for typing, invisible to tooling. Rather than copy the
signatures into the LSP (where they would drift silently), a `store_ops` module in
`bynk-check` enumerates them, exactly as ADR 0063's `kernel_methods` does for the
value kernels — same shape, same rationale, same tooth: a drift test drives every
listed operation through the **real checker** on a `store` field of the matching
kind and fails if any is rejected as `unknown_op`, so the table cannot list a
phantom. Signatures are generic in the kind's `K`/`V`/`T`; the field's declared
kind grounds them at the hover site, so it rides along in the rendering.

**D6 — The regression fixtures sit at reference offsets, in the file the issue
reproduces in.** The gaps existed *because* every hover test used a declaration
offset. Fixtures that repeat that choice would repeat the blind spot, so these
assert against real `diagnose_project` output over `examples/todo/src/todos.bynk`,
at the offsets from the issue.

**D7 — The rung order has one definition, and the tests call it.** Gap B was a
*fall-through* bug: the defect was not in any rung but in which one got to answer.
That makes the order the behaviour, and behaviour a test replicates is behaviour
no test pins — reorder the handler and a replica still passes while the bug
returns. So the ladder moves out of `Backend::hover` into a pure
`hover::hover_content`, and `Backend::hover` becomes transport: resolve the
position, gather the round's tables and the live buffer, package the result. The
tests drive the real function; hoisting the locals rung above the index rung fails
them. The ladder keeps the **snapshot** and the **live buffer** distinct rather
than collapsing them — the index rungs read the analysed snapshot its tables' spans
index into, while the lexical rungs read the live buffer, which is what makes
hover work mid-edit.

## Consequences

- Store/key fields are still **not index symbols**. Hover now covers their
  references by name (D2/D3/D4), but `references`, `rename`, and
  `documentHighlight` over a state field remain unsupported — indexing them
  (a `SymbolKind` plus recorded edges at the checker's resolution sites) is the
  larger change this increment deliberately does not make. D3's scoping and D4's
  shadowing rule are the price of resolving by name; indexing would retire both.
- `store_ops` lists only the **entry** operations. A `Log`'s time-window roots are
  listed (they are `Log` operations), but the general lazy-`Query` vocabulary they
  feed into is the kernel `Query` surface, not a store operation, so a cursor on
  `items.entries.filter` still resolves nowhere. `Queue` is in the storage-kind
  catalogue with no dispatched ops, so it registers none.
- D1's principle is applied to `Field` only. The sibling compound-key kinds ADR
  0069 introduced — `Method`, `CapabilityOp` — fall through the same way, for the
  same reason; giving them arms is an obvious follow-on, not done here.
- **`Actor` is the same gap, found while measuring D1:** an actor name (`by u:
  User`) resolves to an `Actor` key that `describe_item` has no arm for, so
  hovering the actor in `examples/todo` yields nothing at all. Pre-existing and
  outside #611's three gaps, so it is filed rather than fixed here — but it is the
  clearest evidence that the renderer, not the ladder, is where these are missing.
- The registry's drift test pins operation **names** in one direction only: it
  catches a phantom entry, not an omission (an op added to a `check_store_*_op`
  arm later leaves the table silently under-listing, which degrades to a *missing*
  hover), and it does not check the signature **strings**, which the checker never
  reads. `kernel_methods` (ADR 0063) carries the identical limits; both module
  docs now say so rather than implying the pin is total.
- The value-receiver method hover the issue raises as the broader form of gap C
  (`xs.fold` on an ordinary value) is **not** addressed. Store ops are dispatched
  structurally off a declared field, which is what makes them resolvable without
  typing the receiver; a general value-receiver hover needs the receiver's type,
  i.e. signature help's rewrite-and-re-analyse path.
