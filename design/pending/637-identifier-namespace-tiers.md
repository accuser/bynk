---
level: minor
changelog: Reject redeclaring a built-in type name (`List`, `Query`, `QueueResult`, …) with `bynk.resolve.reserved_builtin_type`; document keywords as three tiers
---

## ADR: builtin-type-name-tier
title: Built-in type names are a named, reserved tier; redeclaring one is an error
summary: Name the three identifier-reservation tiers and reject `type <builtin> = …`

**Context.** Bynk's identifier namespace had three de-facto reservation tiers
with no shared representation and contradictory documentation:

1. **Hard keywords** — reserved lexer tokens, rejected as identifiers by
   `expect_ident`.
2. **Contextual keywords** (`on`, `suite`, `case`) — reserved lexer tokens the
   parser deliberately re-admits as identifiers outside their one keyword
   position, so they are valid field and parameter names.
3. **Built-in type names** (`HttpResult`, `QueueResult`, `List`, `Map`, `Query`,
   `Stream`, `Connection`, `History`) — not lexer keywords at all; the type
   parser dispatches on them by identifier text in `parser/types.rs`.

The generated keyword reference claimed every listed word was un-usable as an
identifier — false for tier 2, which it listed — and omitted tier 3 entirely.
The lexer↔registry↔markdown drift guards could not see either divergence,
because it lived in the parser, not the registry.

Worse, tier 3 had a correctness defect. A user could write `type Query = String`
and it compiled clean; the moment they referenced the alias, the parser
intercepted the name as the built-in and reported that *their own* type needed a
type argument. `type QueueResult = String` was worse still: the non-generic
built-in arm returned immediately, silently discarding the user's declaration,
so a reference reported that `QueueResult` was not `String`. No checker
diagnostic rejected redeclaring a built-in.

**Decision.**

- Name the three tiers and give tier 3 a single source of truth:
  `keywords::BUILTIN_TYPE_NAMES` plus `is_builtin_type_name`. The keyword
  reference renders all three tiers with prose true of each; the contextual tier
  (`RESERVED_CONTEXTUAL`) is single-sourced and consumed by both the parser's
  drift guard and the reference.
- **A `type` declaration may not reuse a built-in type name.** The resolver
  emits `bynk.resolve.reserved_builtin_type` at the declaration site, rather than
  accepting it and failing incoherently — or silently — at the reference. The
  eight built-in type names are thus reserved in type position, consistent with
  base types and other reserved keywords (which are already rejected at parse
  time by `expect_ident`).
- A drift guard ties `BUILTIN_TYPE_NAMES` to the exact set of names the type
  parser dispatches on, so the registry cannot fall behind the parser.

**Consequences.** The reference page is accurate for the first time, and the
silent-shadow / incoherent-reference behaviour is replaced by one actionable
diagnostic at the declaration. The change is additive for every conforming
program: no valid program declared a type named after a built-in, since such a
declaration was already unusable.

The deeper cleanup — parsing named and applied types uniformly and resolving
built-ins semantically, which would remove the string-comparison dispatch in
`parser/types.rs` and collapse tier 3 into ordinary type resolution — is
deferred. This increment brings tier 3 into the registry and stops the
user-facing defect without that structural change.

_Provenance: a third-party language review flagged the identifier namespace as
having hidden and contradictory reservations; verified against the compiler with
live `bynkc check` runs._
