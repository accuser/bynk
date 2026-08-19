# 0371 — Two owner-side accessors — `ParsedFile::declares_messages()` and `cron_and_queue_triggers()` — replace raw AST walks in `project.rs`

- **Status:** Accepted (v0.249.23)

summary: Continues Phase G of the #1137 retirement plan — both walks read data `project.rs` never owned, just happened to hold a reference to

**Context.** `#1137`'s retirement plan (`design/tracks/the-ir.md` §6b, Phase G) targets
`project.rs`'s remaining `bynk_syntax::ast` imports. Two more sites fit the pattern P6.42's own
`SourceUnit::name()` finding established: a raw `match`/predicate over a type `project.rs` doesn't
own, sitting in `project.rs` only because that's where the surrounding emission logic happened to be
written, not because the data belongs there.

1. A `messages`-bundle emitter needs one predicate — does this file declare a `messages { … }` block
   — to decide whether to inject the `bynk.locale` `render` fallback import. `project.rs` matched
   `pf.items()` against `CommonsItem::Messages(_)` itself; `pf: &ParsedFile` already owns `items()`
   (`bynk-project/src/discovery.rs`), so the predicate belongs beside it.
2. A context's own `wrangler.toml` compose entry needs its cron expressions and queue names, sorted
   and deduped. `project.rs:2327-2350`'s own comment already recorded that this loop was "relocated,
   unchanged in substance" out of `emit_wrangler_toml` in a prior slice (#1191) specifically so *that*
   function's file needed no AST match — leaving the walk itself sitting one level up, over
   `table: &UnitTable`, a type `project.rs` doesn't own either.

**Decision.** `ParsedFile::declares_messages(&self) -> bool` added next to `items()`
(`bynk-project/src/discovery.rs`) — the predicate a caller needs, not the raw match. `project.rs`'s
`if pf.items().iter().any(|it| matches!(it, CommonsItem::Messages(_)))` becomes `if
pf.declares_messages()`.

`bynk_check::symbols::cron_and_queue_triggers(table: &UnitTable) -> (Vec<String>, Vec<String>)` added
next to `discover_event_subscribers` — a pure function of `table`, carrying forward the exact
same sort+dedup the inline loop did. `project.rs`'s loop becomes one call.

**Consequences.** `ast_importers`: **unaffected (6)** — `project.rs` still counted on its remaining
names (`Block`, `CommonsItem`, `FnDecl`, `ServiceProtocol`, `TypeDecl`, `TypeRef`, `Visibility` —
`HandlerKind` is now gone from the import list entirely, since its only remaining use was this cron
walk); Phase G continues in P6.47–P6.49. No behavioural change: both moves are mechanical, and
`cron_and_queue_triggers`'s own sort+dedup is byte-identical to the loop it replaces. Verified:
zero-diff bless over the full e2e fixture corpus, `cargo test --workspace`, `cargo clippy --workspace
--all-targets`, `cargo fmt --all -- --check`.
