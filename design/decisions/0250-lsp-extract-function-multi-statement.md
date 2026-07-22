# 0250 — LSP extract-function accepts a multi-statement selection

- **Status:** Accepted (v0.217)

**Context.** ADR 0245 shipped extract-function scoped to a single AST
expression, and named multi-statement block extraction as deferred future
work (issue #813): selecting a contiguous run of statements — not just one
expression — and lifting the whole run into a new top-level `fn`.

Two questions ADR 0245 left open for this slice: what a tail-excluded run's
new `fn` returns, and what the call site looks like once it isn't a bare
expression.

**Decision.**

1. **Selection shape.** The selection's span must align exactly with
   statement boundaries within a single block: its start equal to some
   statement's start, and its end equal either to another statement's end
   (the run stops before the tail) or to the block's own tail's end (the run
   includes it). The requested range is whitespace-trimmed at both ends
   first, since a real "select these lines" editor gesture commonly pads
   onto surrounding blank space — that padding is not a partial-statement
   selection the way clipping actual statement content would be. A
   selection that doesn't align this way falls back to the existing
   single-expression algorithm unchanged. The search descends into a nested
   block (an `if`/`match` branch) before trying to align at the outer
   level, mirroring the single-expression algorithm's own descend-first
   policy.
2. **Return type when the tail is excluded.** The new `fn` returns `()`,
   with no explicit tail — the same implicit-unit-tail shape the parser
   already synthesises for any statements-only block (v0.146, ADR 0170) —
   unless the run itself contains a `~>`/`do`/`<-` statement (checked
   recursively through nested `if`/`match`/block bodies), in which case it
   returns `Effect[()]` instead, so those forms stay legal in the lifted
   body. When the tail is included, the return type is the tail's own type,
   exactly as the single-expression case already computes it.
3. **Call-site form.** Bynk has no expression-statement form, so a
   tail-excluded run's call can't stand alone as `extractedFn(args)` the way
   a bare-expression selection's can. It becomes `let _ = extractedFn(args)`
   for a `()`-returning `fn`, or `do extractedFn(args)` for an
   `Effect[()]`-returning one — the statement forms Bynk already has for
   "run this for its effect, discard the result."
4. **Two new decline cases.** A `:=` (`Cell` store write) statement anywhere
   in the run declines the action outright: a lifted top-level `fn` has no
   `store` fields, so the write's target would never resolve — this always
   fails to typecheck, not merely a conservative guess. Separately, a
   `let`/`<-` binding the run introduces that is still referenced later in
   the same block (when the tail is excluded) also declines: lifting the
   binding away would strand that downstream reference.
5. **Free-variable synthesis reuses the existing walk.** Free identifiers are
   collected the same way as the single-expression case — `locals_at` at
   each `Ident` reference's offset, external bindings become parameters, the
   ambiguous-shared-name decline still applies — just seeded from every
   selected statement's values (via `statement_exprs`) plus the tail when
   included, instead of one expression.

**Consequences.** No grammar, checker, or emitter change. The capability-free-only
gate from ADR 0245 is unchanged — it now spans the statement run's whole
selection rather than one expression's. `given`-on-`fn` remains out of scope,
as does any handling of `store` accessor methods (`Map.get`/`.put`/etc.)
reached indirectly through a free variable — a pre-existing gap in the
single-expression case, not introduced or widened here.
