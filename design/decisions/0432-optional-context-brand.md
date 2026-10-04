# 0432 — A context's rebrand of a `uses`-commons type carries an optional brand

- **Status:** Accepted (v0.301.1)

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

Every cast that existed only to bridge the required brand is removed, since
each now bridges nothing:
- #527's call-site casts on commons function results (`set_rebrand_info`,
  `rebranded_types`, `commons_imported_fns`);
- the workers entry's `brand_assertion` (`__r.value as unknown as
  handlers.T`) at its five codec-to-handler sites. A commons codec's unbranded
  result now fits the handler's branded parameter directly, so a codec whose
  return type drifts from the handler's parameter fails `tsc` again, which is
  the check that assertion's own doc said it must not disarm;
- the rebrand's value-side forwarders (`of`, `unsafe`, attached methods), which
  returned `__CommonsT.op(…) as unknown as R` and now return the call.

**Alternatives.** Casting every record literal into the brand was tried
first. A plain `as T` is rejected by `tsc` for a nested literal (neither side
converts to the other), so the cast has to go in two steps through the
commons type. Each site also needs the exact set of `__Commons<Name>` imports
a module emits, and a test module needs different spellings. That fixes one
source of unbranded values per call site; the optional brand fixes all of
them at the type.

**Consequences.**
- Emitted output changes only in the brand member's `?` and the removed
  casts.
- Constructing a *sum* variant of a `uses`-commons type in a context is still
  `bynk.context.rebrand_construction`: the rebrand exports no value-side
  constructors. Its registry text now says "sum", not "record or sum".

Proved by:
- the behavioural fixture `1704_rebranded_record_literals`, which covers a
  literal in a test body, a nested literal crossing an agent's state boundary
  in both directions (a second call reloads it through the nested
  deserialisers), and a context building literals, a spread and a commons
  function's result;
- the compile-only fixture `1704_rebranded_generic_record_literal` (a generic
  `Page[T]` built with record syntax);
- `tsc_verify`'s `context_brands_admit_commons_values_and_reject_other_contexts`,
  which stages `1704_context_brands_distinct` with a probe asserting a commons
  value fits `left.Point` and `left.Point` does not fit `right.Point`;
- `event-log`'s new write-path cases.

Each was mutation-checked: with the brand required again, all fail `tsc`.
