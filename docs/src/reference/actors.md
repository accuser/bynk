# Actors

An **actor** is a nominal boundary contract — an authentication scheme plus an
optional sealed identity — that a handler consumes on its **`by` clause**. The
compiler emits the verification at the boundary, before the body runs,
fail-closed. Actors are declared inside a `context`. For task-oriented recipes,
see [Guides — Actors & access control](../guides/actors/index.md).

## Declaration

```bynk,ignore
actor <Name> { auth = <Scheme>(<config>) , identity = <Type> }   // base actor
actor <Name> = <Base> where <predicate>                          // refinement
```

## Schemes

A closed, compiler-known set. `auth = <Scheme>` with keyed config where noted.

| Scheme | Config | Identity | Protocols | Verification |
|---|---|---|---|---|
| `None` | — | `()` | any | none (anonymous; the prelude `Visitor`) |
| `Bearer` | `secret = "<ENV>"` | a string-constructible type | `http` | JWT HS256 over `Authorization: Bearer …`; `sub` → identity; `401` |
| `Signature` | `secret`, `header`, optional `timestamp` + `tolerance` | none | `http` | HMAC-SHA256 over the raw body; `401` |
| `Internal` | — | `()`, or `CallerId` for `Caller` | `call`/`cron`/`queue` | the Service-Binding channel is the assertion |

- **`secret`** values name an environment variable (the source the `Secrets`
  capability reads), not the key itself.
- A `Bearer` actor's **`identity`** must be a context-owned, string-constructible
  type, so the minted value is sealed to the context.
- A `Signature` actor takes **no** `identity` and a handler using it **must** take
  a `body`; it is HTTP-only.

## The `by` clause

```bynk,ignore
by <binder>: <Actor>            // capture the identity as <binder>.identity
by <Actor>                      // verify, capture nothing (optional binder)
by <binder>: <A> | <B> | …      // an ordered sum of peer actors, first-wins
```

- **HTTP requires a `by` clause** (`bynk.actor.missing_by_on_http`); a public
  route writes `by v: Visitor`. The internal protocols default: `call` →
  `Caller`, cron → `Scheduler`, queue → `Producer`.
- **Multi-actor sums** resolve first-wins; the body `match`es the resolved actor.
  Peers are distinguished by scheme (no duplicates), a `None` catch-all is last,
  refinements are not members, and total failure is `401`.
- **Refinement actors** (`<Admin> = <User> where <pred>`) add an authorisation
  invariant over a `Bearer` base: scheme failure is `401`, predicate failure is
  `403`. The predicate is a closed set — `hasClaim("n")`, `claimEquals("n","v")`,
  composed with `&&`/`||`/`!`. An `Admin` is usable wherever its base is.
- **`Caller`** (on `on call`) yields the calling context's qualified name as
  `CallerId`.

## Prelude actors

| Name | Scheme | Identity | For |
|---|---|---|---|
| `Visitor` | `None` | `()` | anonymous HTTP routes |
| `Caller` | `Internal` | `CallerId` | the calling context (`on call`) |
| `Scheduler` | `Internal` | `()` | cron |
| `Producer` | `Internal` | `()` | queue |

## Diagnostics

All `bynk.actor.*` codes are in the [diagnostic index](diagnostics.md) — among
them `missing_by_on_http`, `scheme_not_admissible`, `signature_requires_body`,
`sum_requires_binder`, `duplicate_sum_scheme`, `unreachable_sum_arm`,
`refinement_base_unsupported`, and `refinement_predicate_unsupported`.

The normative rules are [Specification §5.7a](../spec/static-semantics.md); the
emitted verification is [§7.3.4a](../spec/emission.md).
