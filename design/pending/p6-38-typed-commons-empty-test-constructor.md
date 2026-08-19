---
level: patch
changelog: "P6.38: emitter/lower.rs's idempotency_scoping_tests module reads a new TypedCommons::empty() (bynk-check) instead of hand-building an empty Commons/QualifiedName itself -- this module's own last bynk_syntax::ast dependency, per the AST-free test-constructor option P6.33 named. Does not clear emitter/lower.rs: P6.37 already found this file's own remaining production surface (system_http_route_body's TypeRef field, the two HttpMethod::from_ident sites) has no available IR-native alternative, so the file stays counted regardless -- this slice closes only the test-residue half of R6.13's own \"known gap\" paragraph for this file, not the file as a whole. ast_importers unaffected (7)."
---

## ADR: typed-commons-empty-test-constructor

title: `TypedCommons::empty()` (bynk-check) replaces `emitter/lower.rs`'s hand-built empty `Commons` test fixture

summary: Phase E of the #1137 completion plan (`design/tracks/the-ir.md` §6a, P6.38) — closes the test-residue half of this file's own R6.13 gap; does not clear the file, per P6.37's own finding

**Context.** `emitter/lower.rs`'s `idempotency_scoping_tests` module hand-built an empty `TypedCommons`
for its own fixtures, constructing a `Commons`/`QualifiedName`/`CommonsForm` literal directly
(`bynk_syntax::ast` types) — this module's own last, and by P6.37's own landing this file's last
overall, `#[cfg(test)]`-scoped AST reference. P6.33's own re-settling named three options for this
exact residue: an AST-free `TypedCommons` test constructor in `bynk-check`, exception-list growth, or
relocating the test module — deferring the choice to whichever slice actually closed it.

**Decision: option (a).** Added `TypedCommons::empty()` to `bynk-check/src/checker.rs`, an exact,
byte-for-byte port of the hand-built fixture (empty `Commons` in `Fragment` form, every table empty,
a fresh `Types` intern). `bynk-check` already owns both `TypedCommons` and `Commons` — spelling the
AST type there costs nothing against this probe, which only scans `bynk-emit/src`.
`emitter/lower.rs`'s own `empty_commons()` test helper now calls it, dropping its own
`use bynk_syntax::ast::{Commons, CommonsForm, QualifiedName};` entirely.

**Consequences.** `ast_importers`: **7 → 7**, unaffected — P6.37's own investigation already found
`system_http_route_body`'s `TypeRef` field and the two `HttpMethod::from_ident` sites have no available
IR-native alternative (no resolution context at the former's construction site; a Q7-deferred renderer
consumes the latter), so `emitter/lower.rs` stays counted on that basis regardless of this slice. This
closes only the test-residue half of the file's own R6.13 gap — real progress, not a probe-visible one.
Verified by a full zero-diff bless against the entire e2e fixture corpus (a same-shape constructor swap
cannot alter emitted output — nothing outside `#[cfg(test)]` changed) and a full `cargo test
--workspace`, including the three tests `idempotency_scoping_tests` itself carries.
