---
level: patch
changelog: A record literal of a `uses`-commons type built in a context, or in a test of one, now type-checks under `tsc`. So does a `uses`-commons record nested in another crossing a boundary. The context's brand on a rebranded type is now optional, so a plain commons value fits it while another context's brand is still rejected. `event-log` gains write-path tests (#1704)
---

## ADR: optional-context-brand
title: A context's rebrand of a `uses`-commons type carries an optional brand
summary: `T = __CommonsT & { readonly __ctxBrand?: "<ctx>" }` admits plain commons values and still keeps two contexts' views apart; supersedes #527's call-site casts

**Context.** A context that `uses` a commons rebrands each of its types
(#527, v0.4 §6.2):

```ts
export type Event = __CommonsEvent & { readonly __ctxBrand: "events" };
```

The brand was required, so any *unbranded* value failed `tsc` where the
context's type was expected:
- a record literal (`Event { … }` lowers to a plain object);
- the nested field of another rebranded record, which is typed by the commons
  (`__serialise_Line` passed `value.start: __CommonsPoint` to
  `__serialise_Point(value: Point)`);
- a test body's literal, aliased to the unbranded commons type.

`bynkc check` accepted all of these. #527 had patched one source, a commons
function's result, with a cast at each call site. Record literals had no cast,
and a checker comment wrongly said they needed none.

**Decision.** The brand member is **optional**:

```ts
export type Event = __CommonsEvent & { readonly __ctxBrand?: "events" };
```

- A plain commons value (a literal, a nested field, a commons function's
  result) has no `__ctxBrand`, so it is assignable.
- A value carrying another context's brand is not: `"left" | undefined` is not
  assignable to `"right" | undefined`. So the nominal distinction #527 exists
  for still holds.
- A context's value is still assignable to the commons type.

#527's call-site casts on commons function results are removed
(`set_rebrand_info`, `rebranded_types`, `commons_imported_fns`). The optional
brand makes them unnecessary.

**Alternatives.** Casting every record literal into the brand was tried
first. A plain `as T` is rejected by `tsc` for a nested literal (neither side
converts to the other), so the cast has to go in two steps through the
commons type. Each site also needs the exact set of `__Commons<Name>` imports
a module emits, and a test module needs different spellings. That fixes one
source of unbranded values per call site; the optional brand fixes all of
them at the type.

**Consequences.**
- Emitted output changes only in the brand member's `?` and the removed
  `as T` casts.
- Constructing a *sum* variant of a `uses`-commons type in a context is still
  `bynk.context.rebrand_construction`: the rebrand exports no value-side
  constructors. Its registry text now says "sum", not "record or sum".

Proved by:
- the behavioural fixture `1704_rebranded_record_literals`, which covers a
  literal in a test body, a nested literal crossing an agent's state boundary,
  and a context building literals, a spread and a commons function's result;
- the compile-only fixture `1704_rebranded_generic_record_literal` (a generic
  `Page[T]` built with record syntax);
- `tsc_verify`'s `context_brands_admit_commons_values_and_reject_other_contexts`,
  which stages `1704_context_brands_distinct` with a probe asserting a commons
  value fits `left.Point` and `left.Point` does not fit `right.Point`;
- `event-log`'s new write-path cases.

Each was mutation-checked: with the brand required again, all fail `tsc`.
