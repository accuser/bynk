# 0191 — A renderer arm for every resolved index kind; a bare key names a free function

- **Status:** Accepted (v0.166.0; 2026-07-14)
- **Amends:** [[0190]] D1 — not its rule, which stands, but the **measurement** offered for deferring it. 0190 recorded that `Method`/`Actor` "already hover as *nothing*", so enforcing D1 "would buy no user-visible change while risking the `CapabilityOp` case". Re-measured at reference offsets, two of the three rendered a confidently **wrong** hover instead, and `CapabilityOp` was the worst of them rather than the one at risk.
- **Provenance:** #616, filed from the v0.165.0 hover-references increment — 0190's own Consequences filed `Actor` as "the same gap, found while measuring D1" and named `Method`/`CapabilityOp` as "an obvious follow-on, not done here". This is that follow-on, and it closes the kinds 0190 left open.
- **Realises:** ADR 0190 D1's stated direction of travel — "the arms are what will make it true, one kind at a time" — for every kind there is.
- **Relates:** ADR 0069 (the `Method`/`Field`/`CapabilityOp` compound-key index kinds this renders); ADR 0091 (the actor refinement form `actor Admin = Base where …` the actor arm renders); ADR 0063 (the `kernel_methods` registry whose owner-attribution shape the capability-op arm mirrors).

## Context

ADR 0190 D1 fixed the `Field` arm and stated the general rule — **a structural
resolution outranks a name match** — but declined to enforce it, on a
measurement: the remaining resolved-but-unrendered kinds (`Method`,
`CapabilityOp`, `Actor`) were said to fall through to *nothing*, so arms for them
would be tidy rather than load-bearing.

That measurement was taken from the ladder's tail. Read forward from rung 1 it
looks right — `qualified_callee_at` bails on a lowercase receiver, so a `Method`
reference reaches `resolve_label` and gets `None`. But the fall-through does not
run from rung 1 to rung 7; it runs *through rung 4*, the lexical
`describe_symbol(text, name)` over the live buffer, which name-matches the
identifier under the cursor against the file's top-level declarations. Rung 4
answers first, and it answers by name.

Re-measured at reference offsets against real `diagnose_project` output — the
discipline 0190 D6 exists to enforce — the three kinds are not alike, and only
one of them hovers as nothing:

- **`Method` renders the wrong method.** `describe_item`'s `Fn` arm matched on
  `f.name.ident().name`, a method's *bare* name with its type prefix dropped. In
  a file declaring `fn Counter.bump` and `fn Gauge.bump`, every `bump` — the
  `g.bump()` call the index binds to `Gauge.bump`, and `fn Gauge.bump`'s own
  declaration — rendered `Counter.bump`, the first one declared. The index had
  the right answer at rung 1 and the renderer discarded it.
- **`CapabilityOp` renders an operation from another unit entirely.** Rung 7 does
  answer a `Cap.op` reference, via `resolve_label` — and its search is not scoped
  to the project. `bynk.bynk` declares `platform.log.Logger.info(msg: String)`,
  so a context declaring its own `capability Logger { fn info(message: String) }`
  hovered the **embedded stdlib** op: different parameter, different owner, no
  indication either was in play. 0190 called this the case that enforcing D1
  would "risk"; it was the case most in need of it.
- **`Actor` renders nothing** — as 0190 measured — because no later rung knows
  what an actor is.

So the rule 0190 stated was, for two of three kinds, not merely unenforced but
actively inverted: the ladder was resolving a name correctly and then answering
with a guess. The gap between the two is exactly gap B, twice more, and it went
unseen for the same reason gap B did — the fall-through is silent, and the wrong
answer is plausible.

## Decisions

**D1 — The three remaining kinds get their arms; D1 of 0190 is now true of every
kind the index carries.**
`describe_item` gains an `Actor` arm (the `auth` scheme and its config, the
`identity` type, or the refinement form's base and claim predicate — mirroring
`bynk-fmt`'s `format_actor` as `describe_agent` mirrors an agent), a `Method` arm
(matched on `FnName::display()`, which renders the `"Type.method"` key exactly,
so the type prefix disambiguates), and a `CapabilityOp` arm (the op's signature
attributed to its owning capability, as `describe_record_field` attributes a
field). Every `SymbolKind` now has a renderer, so no resolved key is dropped for
want of an arm — which is the only cause 0190 D1's rule speaks to. Rung 1's guard
can still see `None` for reasons that are not a missing arm (a snapshot that does
not parse, a symbol whose defining unit contributes no items), and those still
fall through by design: the guard is not the enforcement, it is the fall-back.

**D2 — A bare key names a *free* function.**
The `Fn` arm now guards on `FnName::Free`. A method's identity is its compound
`"Type.method"` key; matching one on its bare method name was never a resolution,
only a guess between every type declaring that name, and it silently outranked
the index's real answer. `signature_help::resolve_label` already guards its
free-fn path this way — this brings the renderer into line with the resolver
rather than inventing a rule. The cost is narrow and named: a method declaration
in a buffer no analysis round has reached yet hovers as nothing rather than as a
coin-flip between same-named methods, which is the trade D1's principle asks for.

**D4 — The rule gets a tooth now, because this is the second time it bit.**
An arm per kind fixes today's kinds; it does nothing about the next one. Rung 1
guards on the renderer returning `Some`, so a `SymbolKind` added without an arm
falls through to a name match **silently** — which is not hypothetical: it is
exactly how #611 gap B and all three of #616 shipped, twice, with no test failing
either time. A third was a matter of when.

So the tooth is fitted here rather than named as future work, and it is two
halves because either alone is defeatable. A **sweep** drives every key the real
index produces for a fixture declaring all ten kinds through the real renderer
and fails on any that answers `None` — the invariant, and what would have caught
both bugs. A **`declared_name` lookup**, exhaustive over `SymbolKind`, is the
forcing function: a new variant stops the crate compiling until someone names a
declaration for it. The sweep without the match goes quietly vacuous for a kind
the fixture never declares (passing while covering nothing — the sweep's own
failure mode); the match without the sweep proves only that a name was written
down. Together, a new kind cannot reach `main` without either an arm or a
deliberate decision not to have one.

This is the "mechanical pin" the increment would otherwise have deferred. The
argument for deferring — that it is obvious and cheap, so it can wait — is the
argument that already lost twice.

**D3 — A correction is an amendment, not an edit.**
0190's D1 measurement is wrong, and 0190 is not rewritten to hide it. The rule it
stated was right and is what made this findable; the measurement under it was
taken from the wrong end of the ladder. Recording the correction here — with what
was measured, and why the original looked right — keeps the reasoning auditable,
which a silent fix to the older file would not.

## Consequences

- **The `CapabilityOp` arm changes which unit answers.** Where a project declares
  a capability whose name collides with the embedded first-party surface, hover
  now renders the project's own op. Where the index does *not* resolve the
  offset — builtin type statics (`Stream.of`), a refined type's `of`/`unsafe` —
  rung 7 still answers exactly as before; the arm only outranks it where a
  structural resolution exists. An embedded op that *is* in the analysed
  snapshots (`Clock.now` under a `consumes`) now renders through the arm, gaining
  its owner attribution.
- **`bynk-fmt` exports `escape_string`.** "Mirrors `format_actor`" has to be true
  of the escaping too, or it is not mirroring: an actor's `auth = Scheme(secret =
  "…")` config holds the value *unescaped* (the parser resolves `\"`/`\\`/`\n`/
  `\t` at lex time), so rendering it raw emits invalid Bynk inside a ```` ```bynk ````
  fence once the value contains a quote or a backslash. The escaper is now public
  for the same reason `expr_to_string` (v0.123) and `annotation_to_string`
  (ADR 0161) already are — the LSP renders through the formatter's own logic
  rather than a copy that drifts. The regression test pins hover against
  `format_source`'s actual output, not a hand-written expectation, so the two
  cannot silently diverge again.
- **D2's ceiling is the unanalysed buffer.** Rung 4 is the live-buffer fast path,
  and it can no longer answer for a method. Between an edit and the round that
  follows it, a method declaration hovers as nothing; once analysed, rung 1
  answers correctly. Indexing is what makes a method hoverable, which is the
  honest statement of where the capability comes from.
- **The rule is now a property, and D4 keeps it one.** Rung 1 still *guards* on
  the renderer returning `Some` — the ladder is unchanged, and a resolved key
  with no arm would still fall through silently at runtime. What changed is that
  it can no longer reach `main`: the sweep fails and the exhaustive `match` does
  not compile. The pin is in the **tests**, not the type system, so it holds for
  kinds the fixture declares; a kind the index starts producing from a source the
  fixture has no analogue of would need the fixture extended, which is what D4's
  `declared_name` break prompts.
- **`describe_item` and `find_declaration_span` have now diverged.** The latter
  still matches a method on its bare name (go-to-definition from a bare
  identifier depends on it), so the two functions no longer agree on what a bare
  name means. The divergence is deliberate and is the reason D2 is stated as a
  decision rather than left as a code detail.
- The fixtures follow 0190 D6 — reference offsets, real projects, real
  `diagnose_project` output. `examples/todo` carries the actor (`by u: User`, the
  issue's reproduction); the `Method`/`CapabilityOp` cases borrow the compiler's
  own positive fixtures, which declare things `examples/todo` has none of. Each
  test pins the *wrong* answer it replaces, not just the right one — a fixture
  asserting only `Gauge.bump` would pass against a renderer that had never been
  broken, and this one was.
