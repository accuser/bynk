---
level: minor
changelog: "A service's `cors`, `security` and `limits` policies must open its body in that order, before any handler (#1784). The compiler accepted them anywhere, even after a handler, while the editor's tree-sitter grammar and the specification required this order, and `fmt` silently rewrote a file into it. A policy out of order or after a handler is now the parse error `bynk.parse.policy_order`; move it up"
---

## ADR: service-policy-order-is-enforced
title: A service's policies open its body in the order cors, security, limits, and the compiler enforces it
summary: bynk.parse.policy_order rejects a cors/security/limits policy that is out of order or follows a handler

**Context.** [[0159]], [[0164]] and [[0165]] added a `from http` service's `cors { }`, `security { }` and `limits { }` policies "in header position before the handlers". The tree-sitter grammar's `service_decl` encodes that as three optional sections in a fixed order, then the handlers, and the specification renders that production (§4.4.1a). The compiler's own parser instead parsed each policy as one arm of the loop that parses handlers, so it accepted the sections in any order and anywhere in the body, rejecting only a duplicate. The two parsers therefore disagreed: source that compiled was a parse error in the editor, and `bynk fmt` rewrote it into the canonical order, so `fmt --check` failed a file `check` accepted. The cross-parser conformance test never compared service bodies (#1784).

**Decision.** The compiler enforces the grammar's order. A policy that follows a handler, or one that follows a policy meant to come after it (`cors` after `security` or `limits`; `security` after `limits`), is the parse error `bynk.parse.policy_order`. A duplicate keeps its own code (`bynk.parse.duplicate_cors` and so on), checked first. Nothing changes for source already in the canonical order, which is every file `fmt` has written.

**Consequences.** Source relying on the leniency stops compiling, a breaking change for a pre-1.0 `minor`. None in this repository did: no example, fixture or doc block placed a policy out of order or after a handler. The fix is mechanical: move the section up. `bynk fmt` can no longer make it, since such a file no longer parses. Rejected: relaxing the grammar to accept any order. That keeps a second, non-canonical spelling of every service that `fmt` would rewrite anyway, and makes the specification describe a looser surface than the canonical one. Two negative fixtures pin the new error, and the conformance test's totality pass requires both parsers to reject them.
