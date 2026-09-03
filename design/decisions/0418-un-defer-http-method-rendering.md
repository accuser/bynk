# 0418 — Un-defer the HTTP-method rendering-signature cutover — P6.51 narrowed it enough to be worth it now

- **Status:** Accepted (v0.289.58)

**Context.** ADR 0355 kept `HttpRoute::method`, the three `emit_http_*_wrapper` signatures,
`derive_allowed_methods`, and the direct `HttpMethod` comparisons in `emitter/workers.rs`/
`emitter/workers_entry.rs` explicitly AST-typed, "until phase 7's printer" existed — the same
Q7-settled dispatch-vs-render split `the-ir.md` used throughout: the *decision* of which wrapper to
call reads the IR (`IrHandlerKind::Http`), but *rendering* stayed on the raw AST value. Two call sites
(`workers_entry.rs:380-384`, `workers.rs:606-613`) built `IrHandlerKind::Http { method, path }` purely
to classify the handler, then discarded that payload and re-destructured `h.kind` via `unreachable!()`
to get the same `method`/`path` back out — the 30 August 2026 post-restructuring review named this
"strictly worse than not using \[the IR\]: it adds a second dispatch that has to stay in sync with the
first."

Phase 7 (`the-typescript-tree.md`, `#1293`) retired 29 August 2026, removing the stated reason to keep
deferring. Investigating the actual size of the deferred surface (`design/tracks/the-ir-cutover.md`
§3.2) found P6.51 had already converted most of it: `IrHandlerKind::Http`'s own `method` field is
`IrHttpMethod`, a documented field-for-field mirror of the AST `HttpMethod`
(`bynk-ir/src/lib.rs:1679-1710`), and `http_handler_method_name_ir` — the `IrHttpMethod` sibling of the
AST-typed `http_handler_method_name` — already existed with four other production callers. What ADR
0355 sized as "a cascade well beyond this slice's own scope" was, by the time of this investigation,
five signature sites and three direct comparisons: `HttpRoute::method`, `emit_http_wrapper`/
`emit_http_sum_wrapper`/`emit_http_oidc_wrapper`, `derive_allowed_methods`, and four
`route.method == HttpMethod::Get` reads (one more than the settling doc counted, found during
implementation: `workers_entry.rs`'s `HEAD`-from-`GET` synthesis check).

**Decision.** `HttpRoute::method` is now `bynk_ir::IrHttpMethod`. The two discard sites read
`IrHandlerKind::Http`'s `method`/`path` directly. The three wrapper signatures and
`derive_allowed_methods` take `IrHttpMethod`; all `HttpMethod::Get` comparisons on route data read the
IR value. `http_handler_method_name` (the AST-typed twin) is deleted now that its last production
caller has converted — its one remaining reference was a test (`project/tests_emit.rs`), repointed to
`http_handler_method_name_ir`.

**Consequences.** Zero emitted-output change (`IrHttpMethod::as_str()` is a verified field-for-field
mirror of `HttpMethod::as_str()`); a full e2e fixture bless is byte-identical. `ast_importers` is
unaffected — `workers.rs`/`workers_entry.rs` still import other `bynk_syntax::ast` names, and this
slice's floor was never about that probe. `#1542`'s slice-status checklist records Slice 1 as done.
