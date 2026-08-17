---
level: patch
changelog: "bynk-emit::emitter::workers_entry's @cache/@limit route-annotation reading moves to bynk-emit::ir (lower_route_cache_ir/lower_route_limit_ir, #1228), so the two remaining hand-rolled ExprKind::DurationLit/Ident/IntLit matches in that file read the same already-resolved-literal IR path lower_policy_ir already established, rather than walking Handler::annotations directly (internal only, no CheckedProgram needed since neither reads a type — byte-identical output, confirmed by a zero-diff bless run against 298_http_caching/300_http_limits)"
---
