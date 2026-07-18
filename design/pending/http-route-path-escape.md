---
level: minor
changelog: A route path containing a backslash, newline, or tab is now escaped through the canonical string escaper at every router emit site — previously only `"` was escaped, so an internal backslash silently drifted the emitted path (`\b` read as backspace) and a trailing backslash escaped the closing quote and failed to compile the Worker (#721)
---

## ADR: http-route-path-escape
title: Route paths flow through the canonical TypeScript string escaper
summary: Every router emit site escapes a route path through `escape_ts_string`, not an ad-hoc quote-only replace

**Context.** A route path is a `GET("…")` string literal, and the lexer permits
that literal to contain `\n`, `\t`, `\\`, and `\"`
(`bynk-syntax/src/lexer.rs`); `validate_http_handler`
(`bynk-emit/src/project/validate.rs`) imposes no character-set restriction. The
path is then embedded verbatim in emitted TypeScript string literals at four
router sites in `bynk-emit/src/emitter/workers_entry.rs` — the per-route
dispatch guard, the CORS-preflight match, and the two `405`/`OPTIONS`
fall-through matches — each of which escaped only `"` via
`path.replace('"', "\\\"")`, unlike every other emitted literal, which goes
through the canonical `escape_ts_string`.

A backslash therefore reached the emitted literal raw. Two verified end-to-end
failures followed. An internal backslash silently changed the router's behaviour:
`on GET("/a\\b")` emitted `path === "/a\b"`, and TypeScript reads `\b` as a
backspace, so the router matched a path the source never declared — a security
and correctness drift. A *trailing* backslash broke compilation outright:
`on GET("/a\\")` emitted `path === "/a\"`, where the lone `\` escapes the
closing quote, producing an unterminated string literal that fails the whole
Worker build — a build-time denial of service reachable from ordinary source.

**Decision.** All four sites route the path through
`crate::emitter::escape_ts_string`, the same escaper every other emitted literal
already uses. It escapes `\`, `"`, `\n`, `\t`, and `\r`, so the emitted literal
always denotes exactly the declared path and can never terminate its own quote.
No lexer, parser, or validator change is made: the path is still captured
verbatim, and the neutralisation lives at the emission sites that embed it.

**Consequences.** A route path can no longer alter the structure of the emitted
Worker or drift the router's matching, regardless of its characters. Paths that
contained none of the escaped characters emit byte-for-byte as before, so the
change is invisible to existing services; only paths with a backslash, newline,
or tab change — from broken or mis-matching output to a correct literal. A
positive fixture with a CORS-enabled service whose paths carry internal and
trailing backslashes pins all four sites and, under `tsc --strict`, guards the
compile-break regression end-to-end.
