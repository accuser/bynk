---
level: patch
changelog: "P6.44: discover_event_subscribers moves to bynk-check::symbols, next to build_cross_context_info -- the sibling function its own doc comment already pointed at. ast_importers: unaffected (6)."
---

## ADR: p6-44-discover-event-subscribers-relocation

title: `discover_event_subscribers` relocates to `bynk-check::symbols`

summary: Continues Phase G of the #1137 retirement plan — owner-side relocation, not a conversion

**Context.** `#1137`'s retirement plan (`design/tracks/the-ir.md` §6b, Phase G) targets
`project.rs`'s remaining `bynk_syntax::ast` imports for relocation to the crate that already owns the
data being walked. `discover_event_subscribers` reads `ServiceProtocol::Events`/`TypeRef::Named` off
every `UnitTable::services` entry across the whole project to resolve each subscriber's event type to
its owning context — data `bynk-check::symbols::UnitTable` already owns, and a resolution
`bynk_check::symbols::build_cross_context_info`'s own `consumed_event_names` field performs the exact
same way one context at a time. `symbols.rs:854-857`'s own comment already named
`discover_event_subscribers` as the function it mirrors, before this slice — the two were already
understood as siblings living in the wrong crate relative to each other.

**Decision.** Move `discover_event_subscribers` verbatim into `bynk_check::symbols`, immediately after
`build_cross_context_info` (the sibling its own doc comment pointed at). `project.rs`'s call site
retargets to `bynk_check::symbols::discover_event_subscribers`; `build_cross_context_info`'s own
cross-reference comment updates from `` `discover_event_subscribers` (`project.rs`) `` to `` `discover_event_subscribers`
below ``, since both now live in the same file. `emitter/events_fanout.rs`'s doc comment (which cites
the function by name, not by path) updates its module reference to the new path.

**Consequences.** `ast_importers`: **unaffected (6)** — `project.rs` still counted on its remaining
names (`Block`, `CommonsItem`, `FnDecl`, `FnName`, `HandlerKind`, `ServiceProtocol`, `TypeDecl`,
`TypeRef`, `Visibility` — `ServiceProtocol`/`TypeRef` both survive this slice via other call sites);
Phase G continues in P6.45–P6.49. No behavioural change: the function's body, including its own
build-to-build-determinism sort (the `HashMap` iteration race it documents), is untouched. Verified:
zero-diff bless over the full e2e fixture corpus, `cargo test --workspace`, `cargo clippy --workspace
--all-targets`, `cargo fmt --all -- --check`.
