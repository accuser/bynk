---
level: patch
changelog: "add ActorSeamIr, a new IR type wrapping bynk-check's five already-resolved actor-seam structs by value, and lower_actor_seam_ir, a narrow reader trying them in the one priority order that's load-bearing (sum ahead of Bearer, since a sum's own first peer can itself be Bearer-schemed); converts emit_service (emitter/emit.rs) and emit_worker_compose's HTTP-dispatch match (emitter/workers.rs), the two call sites that actually branch on which of several resolvers succeeded, from independently re-deriving that branch to one shared, tested function (internal only, byte-identical output)"
---
