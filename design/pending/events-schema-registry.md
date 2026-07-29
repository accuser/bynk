---
level: minor
changelog: "Events track: the cross-build schema registry (bynk.schema.lock) computes each event's schema version from its build history, auto-bumping on additive shape changes and verifying a declared @schema(N) against the computed value."
---

## ADR: events-schema-registry
title: The cross-build schema registry ships as an opt-in, reconcile-before-emission auto-write, verifying (not replacing) @schema(N)
summary: Events slice 3c's design — opt-in auto-write, reconcile before emission, bynk.schema.lock's shape, and why @schema(N) is now verified rather than trusted

**Context.** Proposal #980 (Events slice 3c) closed the gap slice 3b (#978)
deliberately left open: `@schema(N)` shipped as an unconditionally-trusted
assertion, with nothing to verify it against. `design/bynk-design-notes.md`'s
original §7 vision is explicit that this was only half the feature — "the
compiler maintains a schema registry across builds, computing the version
from the type's structural shape... explicit `@schema(N)` annotations...
are available for teams that want to pin versions; the compiler verifies the
declared version against what the schema would otherwise warrant." This
slice is that verification, plus the auto-bump path for events that don't
annotate at all — not a parallel auto-detection feature bolted alongside
3b's assertion.

This is `bynkc`'s first command with committed cross-build state. The only
precedent, `bynk.deploy.lock`, is owned exclusively by `bynk deploy`, never
touched by the compiler.

**Decision 1 — the registry auto-writes, but only when explicitly enabled per
call.** `bynkc compile`'s directory build and `bynk dev`/`bynk deploy`'s
build step turn on `CompileOptions::schema_registry` (default `false`); every
other caller — in-memory builds, the LSP, and critically
`bynkc/tests/e2e.rs`, which compiles hundreds of fixtures **in place** from
`bynkc/tests/fixtures/{positive,negative}/*/` — leaves it off. An
unconditional write keyed off "a compile happened" would litter a
`bynk.schema.lock` into every one of those fixture directories on every
`cargo test`. This mirrors the existing `CompileOptions::contracts` flag
exactly: an opt-in behaviour difference between "real build" and "everything
else," not a new pattern.

**Decision 2 — reconciliation runs before emission; only the write is
deferred.** Emission happens inside `run_checks` (`check_unit_files` →
`build_emit_unit_ctx`/`emit_unit`), where slice 3b's
`EmitProjectCtx.event_schema_versions` is populated. By the time
`compile_project` gets a finished `RunChecks` value back, the TypeScript is
already emitted — so the registry read + reconcile (computing each event's
*effective* version) has to happen earlier in the same pass, right after
`unit_tables` is built and before the per-unit check/emit loop, at the same
point `check_event_subscriptions` already runs a whole-project (not
per-unit) validation. Only the **write** is deferred to `compile_project`,
gated on the final build being fully clean — a schema-registry error, or any
unrelated one from a later phase, must leave the file untouched.

**Decision 3 — the file lives at the project root, TOML, atomic write,
`bynk.deploy.lock`'s exact discipline, reused rather than shared.**
`bynk.schema.lock`, versioned (`version: u32`, no serde default — a registry
missing it is corruption, not a fresh project, the identical argument
`DeployLock` makes), keyed by qualified event name (`<unit>.<EventName>`),
written via temp-file + `create_new` + `sync_all` + rename + directory
fsync, and skipped entirely when the serialised content is byte-identical to
what's on disk (a clean rebuild must not touch the file's mtime or produce a
spurious `git diff`). This is the **third** copy of this exact atomic-write
pattern in the repo (`bynk::deploy::ledger`, `bynk_driver::atomic_write`,
now this). Extraction into one shared helper is not possible without a new
crate below `bynk-emit`: both existing copies live in crates (`bynk`,
`bynk-driver`) that depend on `bynk-emit`, never the reverse. Named as
follow-up debt, not fixed here.

**Decision 4 — the registry's own diff is the evolution report; no separate
artefact.** §7 asks for a build to "emit a schema-evolution report... making
the lineage visible during review." A committed lock file's diff across a
pull request already is that report — the same role `Cargo.lock`'s diff
plays for a dependency bump. Building a distinct report-generation surface
would duplicate what version control already renders for free.

**Decision 5 — the diffing needs its own shallow per-field snapshot, not
`bynk-check/src/contract.rs`'s canonical form.** `canon_named_in` (ADR 0200's
cross-context contract hash) renders a record field as `name: type` with no
signal for default-presence, and deep-expands every transitively-referenced
named type. Neither property fits here: an opaque hash from it cannot tell
an additive change (new field, has a default) from a breaking one (new
field, none) — both perturb the hash identically — and deep expansion would
make an edit to a *referenced* type look like a change to the event itself.
The registry instead stores, per field, its name, a small exhaustive
surface-form rendering of its type (own function, not `bynk-fmt`'s private
`type_ref_to_string` — `bynk-emit` does not depend on `bynk-fmt`, and this
string is compared only against itself across builds, never displayed to a
user, so it does not need to match the formatter's or checker's own
rendering), and whether it carries a default. A shape is reconciled against
its stored entry by this rule:

| Registry state | Shape vs. stored | Result |
|---|---|---|
| no entry (new event, or first compile after this slice lands on an existing project) | — | baseline silently at `@schema(N)`-or-`1`; no diagnostic. Protects every already-shipped `@schema(N)` from becoming an immediate mismatch on the first compile after this slice merges. |
| entry exists | unchanged | effective = stored version. A present `@schema(N)` ≠ stored is `bynk.event.schema_version_mismatch`. |
| entry exists | additive (every new field has a default; nothing removed, retyped, or newly required) | effective = stored + 1. A present `@schema(N)` must equal it (same mismatch diagnostic otherwise); absent, the bumped value becomes the effective version with no annotation required. |
| entry exists | non-additive: a field removed, retyped, added *without* a default, **or one that lost a default it previously had** | hard error, `bynk.event.non_additive_schema_change`, naming the offending field(s). Never a silent bump. |

The "lost a default" case was not in the original design — found only while
writing the behavioural test (`bynkc/tests/events_schema_registry_
behaviour.rs`): removing a field's default without changing its name or type
is invisible to a same-type-set diff, but it is exactly as breaking as never
having had one, since an older wire event that omitted the key (relying on
the default) can no longer decode. `reconcile_one`'s diff checks for it
explicitly, alongside removed/retyped/added-without-default.

A renamed event (this track's prescribed path for an actual breaking
change) leaves its old key stale in the registry with no diagnostic — the
key simply stops appearing in the reconciled document. This is expected,
not an oversight.

**Deferred, not built.** A `--locked`-style flag to fail instead of write
(for a CI job that wants to detect uncommitted registry drift): `git diff
--exit-code bynk.schema.lock` after a build already covers this with no new
CLI surface; a dedicated flag can follow if it turns out to be wanted.

**Consequences.** `env.schemaVersion` is now genuinely computed and
verified, not merely author-asserted — closing the exact gap #978's own ADR
(0298) named as an accepted, temporary gap. `bynkc` gains its first-ever
persisted, committed build artefact outside `bynk deploy`'s ledger; every
non-CLI caller (tests, the LSP, in-memory builds) is unaffected, proven by
the full workspace test run leaving no stray `bynk.schema.lock` anywhere in
the tree. `EventEnvelope`'s doc comment in `bynk-check/src/firstparty/
bynk.bynk` is corrected again, from "author-asserted, not computed" to
describing the registry. Proven at two levels: `bynk-emit::project::
schema_registry`'s own unit tests cover every row of the reconciliation
table in isolation; `bynkc/tests/events_schema_registry_behaviour.rs` proves
the registry actually reaches the `Events.emit` mint site across three real
compiles of one on-disk project — baseline, additive auto-bump (with the
re-emitted TypeScript reflecting it), and a blocked non-additive change that
leaves the committed registry exactly as the prior successful build left it.
