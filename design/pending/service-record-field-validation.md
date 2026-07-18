---
level: minor
changelog: "Record construction in a `service`/`agent` handler body now validates the whole field set — a missing required field, an undeclared extra field, or a duplicate initialiser is rejected (`bynk.resolve.missing_field` / `unknown_field` / `duplicate_field_init`), closing a soundness hole where such a record could cross the HTTP boundary; the checks already governed `fn`/method bodies (#711)."
---

## ADR: service-record-field-validation
title: Record field-set validation reaches service/agent handler bodies
summary: Why the missing/unknown/duplicate-field checks moved into the checker's record-construction path

**Context.** A record literal's *field set* — every declared field present, no
undeclared extra field, no field initialised twice — was validated only by the
resolver's reference walk (`bynk-check/src/resolver.rs`,
`ExprKind::RecordConstruction`). But `resolve_file_record` deliberately skips
`Service`/`Agent`/`Actor` items, so handler bodies never pass through that walk;
they are analysed by the checker alone. The checker's
`check_record_construction` (`bynk-check/src/checker/expressions.rs`) only ever
compared the *types* of fields that were both declared and provided — it
silently ignored an unknown field, never noticed a missing one, and returned the
record's nominal type unconditionally. So an ill-formed record in a service
handler compiled clean and the emitter shipped it: a `type User = { id, name }`
could be returned as `Ok(User { id: 1, bogus: 2 })` — no `name`, an undeclared
`bogus` — and cross the HTTP boundary, directly contradicting the language's
"make illegal states unrepresentable" guarantee at exactly the point it matters
most. The identical literal in an `fn` body was correctly rejected (#711, P0).

**Decision.** Run the field-set checks — missing required, unknown extra,
duplicate initialiser — in `check_record_construction`, once, over the shared
`declared` map so both the non-generic and generic record paths are covered.
The diagnostics reuse the resolver's exact codes and messages
(`bynk.resolve.missing_field` / `unknown_field` / `duplicate_field_init`) so a
service handler and an `fn` reject the identical literal identically. This does
not double-report on `fn`/method bodies: `checker::check` runs only after
`resolver::resolve` returns `Ok` (`bynk-emit/src/lib.rs`), so any resolver-caught
field-set error has already stopped the pipeline before the checker runs — for
handler bodies (which the resolver skips) the checker is the sole backstop. This
is the same seam, and the same fix shape, as mirroring the resolver's
ident-resolution ladder in the checker for handler bodies.

**Consequences.** A `service`/`agent` handler that constructs an ill-formed
record now fails to compile with a spanned diagnostic, closing a boundary
soundness hole rather than emitting the unsound value. The rule is not new —
`fn`/method bodies already enforced it — so no well-formed program changes
behaviour; only records that were already illegal elsewhere are now rejected
everywhere. The recurring lesson holds: an enforcement pass must cover *every*
construction site, not just the one its author first wired up (cf. the held-
resource linearity fixes for match-arm bindings and fn/method bodies).
