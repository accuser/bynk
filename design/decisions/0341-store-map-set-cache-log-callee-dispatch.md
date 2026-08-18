# 0341 — `lower_method_call`'s storage Map/Set/Cache/Log branches read `Callee::Store`/`Callee::Query` instead of `!cx.is_local(&id.name)` — continuing P6.21's own bounded, incremental approach

- **Status:** Accepted (v0.248.9)

summary: Extends the same pattern the first P6.21 PR established for `lower_call` (agent construction, sum-variant construction) to four of `lower_method_call`'s own ~20 branches — the ones with the clearest, most directly precedented `Callee` backing

**Context.** The first P6.21 PR resolved the slice's own open (a)/(b) design question — surgical swap vs. full rewrite — by doing the surgical swap first, only where a branch already has real `Callee` backing, leaving everything else (including all of `lower_method_call`) for later. This PR is that "later," continuing the identical pattern into `lower_method_call`'s own storage-field branches.

**Decision.** Four of `lower_method_call`'s ~20 `if let ExprKind::Ident(id) = &receiver.kind` branches dispatch storage operations on `store Map`/`Set`/`Cache`/`Log` fields, each previously guarded by `!cx.is_local(&id.name)` — a name-matched re-derivation of what the checker's own `Callee::Store{field,op}`/`Callee::Query{field,op,role}` (P6.0) already resolved once, keyed by the call's own `e.id`, not the receiver's bare name. Each branch's guard now reads:

- **`Map`**: `Some(Callee::Store { .. } | Callee::Query { .. })` — `Map` gets both, per `checker.rs`'s own `StoreField::Map` arm (`is_query_op` decides which).
- **`Set`**: `Some(Callee::Store { .. })` only — `checker.rs`'s own `StoreField::Set` arm records only `Store`, never `Query`.
- **`Cache`**: `Some(Callee::Store { .. })` only — same as `Set`.
- **`Log`**: `Some(Callee::Store { .. } | Callee::Query { .. })` — `append` is `Store`; the window-root/general query vocabulary is `Query` (`checker.rs`'s own `StoreField::Log` arm, mirroring `Map`'s split).

Each branch's own **kind-detecting side-table lookup stays unchanged** — `cx.is_agent_store_map`/`cx.is_agent_store_set`/`cx.agent_store_cache_ttl`/`cx.agent_store_log_retain`. These answer a genuinely different question than the one `Callee` settles: given this call really is a store operation on *some* field (now `Callee`-confirmed, not name-matched), *which kind* of store field is it — Map vs. Set vs. Cache vs. Log — the thing that decides which of these four branches is even the right one to take. `Callee::Store`/`Query`'s own `field: String` carries no kind information (that lives only in `bynk-emit::ir::StoreKindIr`, a different module this string-emitter code doesn't consult), so the kind lookup is orthogonal, necessary regardless of how the receiver-detection half is done, and untouched here.

**Left untouched, deliberately.** The held-map branch (real-time held connections over a `Map[K, Connection]`, `agent_held_map_frame`) is a different, narrower concept than an ordinary storage field — not converted here, pending its own check of whether/how it maps onto `Callee`. `Cell` never reaches any of these branches at all: a `Cell` field is bound into ordinary local scope (the string emitter's own `lower_ident`, mirroring the IR pass's v0.81 rule), so `cx.is_local(&id.name)` is already `true` for one and it takes the ordinary local-receiver method-call path — nothing to convert. The remaining ~15 branches of `lower_method_call` (held-map ops, `HttpResult`/`List`/`Map` statics, `Int`/`Float`/`Duration`/`Instant`/`Bytes`/`Stream` parsing, `Events.emit`, local-agent-var dispatch, the `Ty`-keyed kernel-method fallthrough) are untouched — this PR is another bounded increment, not the full P6.21 cutover.

**Consequences.** Verified by a full zero-diff bless against the entire `bynkc` e2e fixture corpus — byte-identical generated output for every fixture, confirming the `Callee`-driven read reproduces every dispatch decision the name-matched code made for every real construct the corpus covers. `ast_importers` unaffected (this file was already counted).
