---
level: patch
changelog: "P6.21 (closing the last gap): emitter/lower.rs's remaining HttpResult/QueueResult branches now read the Callee::Intrinsic sink #1251 added, instead of a bare id.name == HTTP_RESULT check (lower_method_call's qualified-with-args and qualified-nullary forms) or a Ty::HttpResult/Ty::QueueResult type check plus a bare http_variant/queue_variant keyword lookup (lower_ident's bare-nullary form, lower_call's bare-with-args form). The qualified-with-args forms close a real instance of R6.5's name-matched-receiver defect class the same way every other P6.21 slice this session did; the two type-checked forms were already immune to name-matching and are converted for consistency with the rest of this module, not a new defect closure. Verified by a full zero-diff bless against the entire e2e fixture corpus, including test-body fixtures. This closes P6.21's own HttpResult/QueueResult gap in full -- every real dispatch site in emitter/lower.rs for these two built-in result types is now Callee-driven"
---

## ADR: http-queue-result-emitter-consumption

title: The last of `emitter/lower.rs`'s `HttpResult`/`QueueResult` branches read `Callee::Intrinsic` — P6.21's own gap fully closed

summary: Wires the emitter side to the checker-side sink #1251 just added, completing the HttpResult/QueueResult half of P6.21's incremental conversion across all six real dispatch sites

**Context.** #1251 added `Callee::Intrinsic` recording for `HttpResult`/`QueueResult` bare-variant construction at all 8 real checker dispatch sites — purely additive, nothing in `bynk-emit` consumed it yet. This slice is that consumption: six sites across `lower_method_call`, `lower_ident`, `lower_call`, and the field-access-equivalent nullary branch, each still name-matched or type-matched against the bare receiver.

**Decision.** Two of the six sites (`lower_method_call`'s qualified-with-args `HttpResult.Variant(args)` branch, and the field-access-equivalent qualified-nullary `HttpResult.Variant` branch) were guarded by a bare `id.name == HTTP_RESULT` check — a real instance of R6.5's name-matched-receiver defect class, the same one every other branch this session converted closed. Both now read `Callee::Intrinsic`.

The other four sites (`lower_ident`'s bare-nullary `HttpResult`/`QueueResult` forms, `lower_call`'s bare-with-args forms) were already guarded by the expression's own *resolved type* (`Ty::HttpResult(_)`/`Ty::QueueResult`) rather than a bare name — already immune to the shadowing risk the other conversions closed, since a locally-shadowing value can never itself type as `Ty::HttpResult`. Converting these to `Callee::Intrinsic` too is for consistency with every other static-constructor branch in this module (matching this session's own treatment of `Int`/`Float.parse`, similarly already-safe but converted anyway), not a new correctness fix.

**Consequences.** Verified by a full zero-diff bless against the entire `bynkc` e2e fixture corpus, including the test-body fixtures that caught #1247's own regression — confirming `Callee::Intrinsic` reaches every one of these six sites correctly, in every context. This closes P6.21's own `HttpResult`/`QueueResult` gap in full: every real dispatch site for these two built-in result types in `emitter/lower.rs` is now `Callee`-driven. `Events.emit` and `lower_method_call`'s remaining branches (held-map ops, the kernel-method fallthrough) are still open, as is P6.23's own remaining work (bare `QueueResult::Ack`-as-value and the two not-yet-root-caused bugs).
