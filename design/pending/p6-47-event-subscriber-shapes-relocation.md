---
level: patch
changelog: "P6.47: lower_event_subscriber_shapes_ir (bynk-emit/src/ir/lower.rs) absorbs the ServiceProtocol::Events pre-filter guarding lower_service_item_ir, and EventSubscriberShape moves to ir.rs -- project.rs's per-file loop becomes one call. ast_importers: unaffected (6)."
---

## ADR: p6-47-event-subscriber-shapes-relocation

title: `lower_event_subscriber_shapes_ir` absorbs the `Events` pre-filter it guards

summary: Phase G's riskiest slice — moves a live emission-shaping walk, verified against the full fixture corpus including the fixture pinning this exact codepath

**Context.** `#1137`'s retirement plan (`design/tracks/the-ir.md` §6b, Phase G) named this the
riskiest remaining `project.rs` site: a per-file loop over `program.program().commons.items`,
matching `CommonsItem::Service`/`ServiceProtocol::Events` before calling
`lower_service_item_ir` and capturing an `EventSubscriberShape` (two facts a *different* unit's own
composition root later needs to decide whether a cross-context event subscriber wants the envelope
forwarded). The `ServiceProtocol::Events` guard is deliberate, not raw-AST residue —
`lower_service_item_ir` unconditionally lowers every handler's own body, so the guard is a cheap,
structural pre-filter in front of a real cost, not a resurrected AST *read* — but the guard and the
call it guards were living in two different crates.

**Decision.** Moved the whole walk into `bynk-emit/src/ir/lower.rs` (an `AST_IMPORTER_EXCEPTIONS`
file) as `lower_event_subscriber_shapes_ir(program: &CheckedProgram) -> HashMap<String,
EventSubscriberShape>` — same guard, same lowering call, same panic message on the invariant it
already asserted, unchanged in substance. `EventSubscriberShape` itself moves from `project.rs` to
`ir.rs`, next to `ProtocolIr::Events` (the IR node it reads `schema_dispatch` off of) — the natural
consumer-adjacent home the struct's own pre-existing doc comment already implied. `project.rs`'s
per-file loop shrinks to one call:
`event_subscriber_shapes.extend(ir::lower::lower_event_subscriber_shapes_ir(&program))`.

**Consequences.** `ast_importers`: **unaffected (6)** — `project.rs` now counted on five names
(`Block`, `FnDecl`, `TypeDecl`, `TypeRef`, `Visibility` — `CommonsItem`/`ServiceProtocol` both gone
from the import list entirely); Phase G's remaining surface is P6.48–P6.49. No behavioural change:
the guard, the lowering call, the `two_param_handler`/`schema_dispatch` extraction, and the panic
invariant are byte-identical, just relocated. Verified with extra caution, per this slice's own risk
— zero-diff bless over the full e2e fixture corpus, **with `1232_events_envelope_schema_dispatch_bare`
(the fixture pinning this exact `schema_dispatch` codepath) checked individually**, plus `cargo test
--workspace`, `cargo clippy --workspace --all-targets`, `cargo fmt --all -- --check`.
