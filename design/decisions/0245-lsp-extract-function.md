# 0245 — Track #800 settles: LSP extract-function ships capability-free-only

- **Status:** Accepted (v0.216)

**Context.** Issue #303 asked for two `codeAction` refactors; extract-variable
shipped in ADR 0240 (v0.214, #802), and extract-function was split to a
feature track (#800) because it crossed a real blocker: `given Cap`
capability clauses exist only on `Handler` and `Provider` in the AST —
`FnDecl` carries no such field, so lifting a capability-using body into a
plain `fn` isn't expressible without a language change. The track listed
three candidate surfaces: (1) add `given` to `fn`, (2) stay capability-free
only, or (3) target a different shape entirely (e.g. a method on the
enclosing type).

Re-investigating before settling found candidate 3 doesn't actually hold as
stated: a type's methods are checked through the exact same `check_fn` path
as free functions (`CapabilityCtx::default()`, no `given`), so "extract to a
method" only sidesteps the language change for a handler/provider-local op,
not the general case the issue's own wording implied. Candidate 1 is a real,
separate language-surface change (parser + checker + resolver + emitter),
out of proportion to a `codeAction` increment. Candidate 2 is directly
actionable today: `bynk-check`'s capability-requirement ledger
(`RequirementSink`/`Requirement`, ADR 0127) already records every capability
use in `fn`/method bodies too (it has since its introduction — not something
that needed extending), so "does this selection use a capability" is a
zero-cost query against data the ghost `given` inlay hint already reads.

**Decision.** Ship extract-function now, capability-free-only, as a single
ordinary increment rather than a multi-PR track:

1. **Capability-free-only, gated by the existing requirement ledger.** The
   action declines (offers nothing) whenever any recorded `Requirement`'s
   site falls within the selection — covered or not, since a plain `fn` has
   no `given` to cover it once lifted. No new ledger, no language change;
   `fn` stays exactly as capability-free as it is today.
2. **Free variables from `locals`, not a new binding pass.** The selection's
   `Ident` references are walked via `expr_children` (the same exhaustive
   child iterator extract-variable's `locate` already uses); each
   reference's nearest enclosing binding — `locals_at` at the reference's own
   offset, the same query hover/completion/navigation already share — is
   free when it sits outside the selection. An identifier with no local
   binding (a top-level `fn`, a capability, a type name) is left alone,
   already resolvable at the new `fn`'s own scope. Two distinct outer-scope
   bindings sharing a free variable's name (a rare nested-shadow collision)
   declines rather than guessing which one to thread.
3. **Types from the existing Ok-path captures, not re-inference.** The new
   `fn`'s return type comes from `expr_types` (an exact span match against
   the selected expression); each parameter's type comes from the matching
   `LocalBinding.ty` (already rendered Bynk surface syntax). Both are
   Ok-path-only (ADR 0063's clean-file ceiling) — a file with an unrelated
   error elsewhere yields no action rather than a guessed type, the same
   ceiling completion and hover already accept.
4. **Scope: a single expression, `Commons`/`Context` files only.** The
   selection resolves to one AST expression node exactly like
   extract-variable (not an arbitrary run of statements) and only within a
   top-level `fn`/`Provider`/`Service`/`Agent` item — a new top-level `fn`
   needs somewhere top-level to live, so `Adapter` (no Bynk bodies) and
   `Suite` test cases (no enclosing top-level item to insert above) offer
   nothing. Multi-statement block extraction is real future work, tracked
   separately (issue #813) rather than expanding this increment's scope.
5. **No standalone track doc.** Per the feature-track lifecycle, a settling
   PR normally adds a persistent `design/tracks/<slug>.md` before the slices
   are cut. Since this investigation converges on "no language change" and
   the whole mechanism fits one increment, adding then immediately retiring
   a track doc in the same PR is ceremony with no reader benefit — this ADR
   is the settling record, and the increment ships alongside it, closing
   #800 directly (the same shape ADR 0240 used for extract-variable's own
   settling-in-place decisions).

**Consequences.** `textDocument/codeAction` offers a second
`CodeActionKind::RefactorExtract` action, "Extract function `extractedFn`",
alongside extract-variable, on the same selection. No grammar, checker, or
emitter change — `fn` gains no new surface; the action simply declines
outside its narrow, safely-inferable cases. The `given`-on-`fn` question
(candidate 1) stays open as a genuinely separate, larger increment if a
future need for capability-using extraction emerges — this decision doesn't
foreclose it, it just doesn't take it on here.
