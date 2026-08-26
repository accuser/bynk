---
level: patch
changelog: A service handler's subscriber-filter/schema-gate prologue no longer shifts its own body's source-map lines
---

## ADR: emit-service-prologue-checkpoint-rebase
title: emit_service rebases body_smb's checkpoints when it prepends a prologue
summary: insert_str at offset 0 now shifts the sub-builder's own recorded checkpoints by the same byte count

**Context.** `emit_service` lowers a service handler's body into a local
`body_out` buffer, recording each statement's checkpoint into a sub-builder
`body_smb` at the byte offset the statement starts at *within that buffer*.
After lowering completes, a handler that needs a subscriber-filter prologue
(`on event ... from Events(Pattern { .. })`) or a schema-gate prologue (`via
schema(N)`) — or both — does `body_out.insert_str(0, &prologue)`, which
shifts every byte of the body's own already-lowered text to the right by
`prologue.len()` (#1363). `body_smb`'s checkpoints, recorded before the
insert, kept their pre-insert offsets.

`SourceMapBuilder::merge` (`bynk-ts/src/source_map.rs`) resolves each
checkpoint's line via `line_of_offset` against the *same* `body_out` text it
is handed — which, by the time `merge` runs, is always the post-insert
buffer. An un-rebased checkpoint's offset therefore lands short of where its
statement actually sits, one line short per inserted prologue line, so
`merge` attaches its source span to the wrong generated line — confirmed by
a regression test that resolves a real statement to a neighboring
statement's source line when a two-prologue handler is compiled without the
fix.

**Decision.** Give `SourceMapBuilder` a `shift_checkpoints(delta: usize)`
method that adds `delta` to every recorded checkpoint's generated-buffer
offset, and call it in `emit_service` immediately after each
`body_out.insert_str(0, &prologue)`, with `delta = prologue.len()`. A
handler with both prologues shifts twice, once per insert, matching the two
lines actually prepended. `SourceMapBuilder::merge` itself is unchanged —
the fix keeps a sub-builder's checkpoints valid against its own buffer's
current contents rather than teaching `merge` about a splice-order
dependency.

**Consequences.** `bynkc/tests/source_map_bodies.rs` gained
`service_handler_with_subscriber_and_schema_prologue_maps_per_statement`,
which compiles a real subscriber service carrying both prologues and asserts
each of the handler's own statements resolves to its exact `.bynk` line —
failing (confirmed by reverting the fix locally) without the rebase and
passing with it. No other `body_out.insert_str` call site exists in
`bynk-emit` (`emit_service` is the only caller of this prologue technique),
so this closes #1363 in full; a future prologue-prepending site should reach
for `shift_checkpoints` from the start rather than rediscovering this bug.
