---
level: patch
changelog: Arc E design pass — settles `emitter/serialisation.rs`'s conversion status (it converts, using the established Arc C pattern, unlike `emitter/lower.rs`'s permanent exclusion) and names an 8-slice decomposition order
---

## ADR: arc-e-serialisation-rs-conversion-decomposition

title: `emitter/serialisation.rs` converts to real `bynk-ts` nodes using the Arc C pattern, decomposed into four bounded clusters plus two caller-side wrappers

summary: Unlike `emitter/lower.rs` (ADR 0391), `serialisation.rs` is bounded by a closed, small vocabulary — three `TypeBody` variants plus a fixed set of built-in generics — so it converts rather than joining `lower.rs` as a second permanent opaque exception

**Context.** #1319 (the second correction to Arc C's own ordering) removed `serialisation.rs` from
Arc C's slice sequence, deferring the open question of whether it converts at all to its own
dedicated design pass. `serialisation.rs` is 1,951 lines, 241 of `ts_writes`'s 1,072 sites (the
single largest remaining contributor), 11 `pub(crate)` entry points. A direct read of every
function body and every real caller resolves it into four clusters: the codec-writer family
(`emit_helpers_for_owner[_qualified]` → per-`TypeBody`-shape dispatch, 131 sites, builds whole
`serialise_<T>`/`deserialise_<T>` function declarations), expression serialise/deserialise builders
(`serialise_expr[_via]`/`deserialise_expr[_via]`/etc., 23 sites, returns one inline expression,
fully independent of the other three clusters), generic-instantiation helpers
(`emit_generic_helpers[_qualified]`, 79 sites, mirrors cluster 1's dispatch over `Result`/`Option`/
`List`/`Map`), and a second, independent, text-based type renderer (`ts_type_ref_qualified`/
`ts_inner_type`, 8 sites, duplicating what P7.9 already solved for the unqualified case). A real
dependency edge exists between clusters 1 and 3: `emit_field_deserialise`/
`emit_field_deserialise_wire` (the tail of cluster 1) is called from both.

**Decision.** `serialisation.rs` converts, using the Arc C `out: &mut String` → real-node signature
change pattern already applied ~15 times, not left as a second `lower.rs`-style permanent
exclusion — it is bounded by a closed vocabulary (three `TypeBody` variants, a fixed generic set),
not the open Bynk expression grammar `lower.rs` covers. Decomposition order, leaf-to-root: (1) the
expression-builder cluster (fully independent, convert first); (2) fold cluster 4's duplicate type
renderer into the already-converted `ts_type_ref` machinery and delete it; (3) the shared leaf
(`emit_field_deserialise[_wire]`), ahead of both clusters that depend on it; (4) cluster 1's own
`TypeBody`-shape dispatch, by variant (refined/opaque, record, sum — likely 3 slices), gated on
step 3; (5) cluster 3, the generic-instantiation helpers, gated on steps 2 and 3; (6) the
caller-side wrappers (`emit_boundary_helpers`/`emit_json_codec_helpers`/
`emit_consumed_context_helpers` in `emitter.rs`), threading real `Vec<TsDecl>` into `emit_project`'s
tree instead of an `&mut String` — load-bearing, not optional, since skipping it would leave steps
4/5's real nodes with nowhere to go but back into an opaque `Verbatim` wrap; (7) the legacy
`emitter::emit`/`Compiled.ts: String` boundary (`bynk-emit/src/lib.rs`, used by `bynk-driver`/`bynk
check`), adapting to print the now-real tree at its own public API edge — may fold into step 6.
Roughly 8 slices total.

**Consequences.** Names a concrete, scoped path to retiring `ts_writes` to (at most) an unknown-but-
currently-empty residual, closing the single largest named remainder against that probe. Does not
retire the track by itself — `ts_any` and `verbatim_sites` have their own remaining sources outside
this file (`emitter/lower.rs`'s residual `any` sites, `project.rs`'s 3 genuine `Verbatim::new`
sites, R7.7's runtime-error-type dependency) and need their own separate settling before
`design/tracks/the-typescript-tree.md` §12's retirement PR is possible.
