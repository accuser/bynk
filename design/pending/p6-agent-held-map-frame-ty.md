---
level: patch
changelog: "bynk-emit's emit_agent now resolves a held store Map field's own frame type through a new TyId-level held_frame_ty helper (recursing through Option/Effect, mirroring the checker's own type_ref_is_held) instead of a bare TypeRef::Connection match — fixing a real, previously-uncovered gap where a Map[K, Option[Connection[F]]] value rendered the whole Option<Connection<F>> wrapper instead of unwrapping to F (internal only; byte-identical output for every existing held-map fixture, all of which use a bare Connection)"
---
