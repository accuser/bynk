# 0088 — The `by` binder is optional (amends 0082)

- **Status:** Accepted (v0.50)
- **Spec:** `syntactic-grammar.md` (`by_clause`), `static-semantics.md` (§5.7a)
- **Amends** ADR 0082 (the `by` clause), which made identity a named binding. This relaxes the
  *binder*, not the contract.

## Context

ADR 0082 required HTTP handlers to name their actor (`by v: Visitor`) and bound the verified identity
to the name, read as `name.identity` — deliberately a *named binding, never ambient* (the anti-pattern
was Yesod's `requireAuthId` reaching into hidden state). But the anonymous `Visitor` (the `None`
scheme) yields a `()` identity: there is nothing to read, so the binder is dead weight on every public
route. The same arises wherever a handler verifies but does not need the identity (a Bearer gate that
only cares the caller is valid).

## Decision

The `by` binder is **optional**: `by <Actor>` declares-and-verifies the contract without capturing the
identity; `by <name>: <Actor>` captures it (read as `name.identity`). This is a ceremony reduction,
not a return to ambient authority: a binder-less `by Visitor` still declares the actor and scheme
explicitly in the handler signature and verifies before the body — you decline to *capture* an
identity, not to *declare* the contract. So 0082's "no ambient authority" intent holds.

- **All schemes**, uniformly. `by User` (Bearer, binder-less) is a legitimate verify-and-discard
  gate: the token is still verified fail-closed; the identity is simply not minted or threaded.
- **`_` is not admitted as a binder.** Omitting the binder is the one way to express "anonymous"
  (`by _: Actor` is rejected with a fix-it pointing at `by <Actor>`), so there is exactly one spelling
  per intent.
- **HTTP still requires a `by` clause** (`bynk.actor.missing_by_on_http`); only the binder is optional.
  Per-protocol default-actor inheritance (omitting `by` entirely on non-HTTP) is unchanged.

## Consequences

Public routes lose the dead binder (`by Visitor`), and verify-and-discard becomes expressible.
Emission is unchanged for existing named handlers (byte-identical); a binder-less handler mints no
identity — for Bearer the seam still verifies fail-closed but threads nothing into `deps`. One
grammar tweak (optional binder, one-token `:` lookahead) and `ByClause.binder: Option<Ident>`. Future
actor slices inherit the optional binder. The docs/worked examples adopt the terser `by Visitor`.
