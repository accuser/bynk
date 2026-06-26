# 0094 — Error-tolerant receiver typing: best-effort partial expr_types in Analyse mode

- **Status:** Accepted (LSP tooling track, slice 4)
- **Spec:** `design/bynk-lsp-spec.md` §3.15
- **Realises:** the LSP tooling track, slice 4 (G6); lifts the clean-file ceiling that ADR 0093 D4 confines to the value-receiver cell.

## Context

Value-receiver completion (`x.method`/`x.field`) and signature help both type the
receiver through one shared path — `type_receiver` in `bynk-lsp/src/main.rs`:
rewrite the buffer so it parses, re-analyse it (`Mode::Analyse`), and read the
receiver's type from the file's `expr_types`. That path carries the **clean-file
ceiling**: when the file has *any* error, it returns nothing, so both features go
silent — precisely mid-edit, when the buffer most often has an error somewhere.

The ceiling is sharper than "the file must parse." `checker::check_record`
(`checker.rs`) types **every** well-typed sub-expression as it goes, accumulating
them in `expr_types`, then discards the whole map on a single final gate:

```rust
if errors.is_empty() { Ok(TypedCommons { expr_types, .. }) } else { Err(errors) }
```

In `Mode::Analyse`, `project.rs` records a file's `expr_types` only on the Ok
path (past the per-file `Err(errs) => continue`), so one unrelated type error
anywhere in the file throws away the types of every well-typed expression in it —
including the receiver under the cursor, which on its own types fine.

The track speculated a strategy fork: **longest-clean-prefix** typing vs. a
**last-good-per-binding snapshot**. Reading the checker dissolves the fork — the
types are *already computed*; they are merely withheld by the final gate. A third
option dominates both.

## Decision

**In `Mode::Analyse`, record a file's `expr_types` even when the file has errors —
a best-effort partial map.** Surface the per-expression types the checker already
computed instead of discarding them on `errors.is_empty()`. Receiver typing then
succeeds whenever the receiver expression *itself* type-checks, regardless of
errors elsewhere in the file.

- **Reject longest-clean-prefix.** It needs repeated re-checks of truncated
  buffers, an error *after* the cursor still truncates the prefix, and it is
  imprecise exactly around the edit point. More machinery, worse coverage.
- **Reject the last-good snapshot.** Its types are stale, and — worse — its
  binding/expression spans mis-position the moment an edit shifts offsets, which
  is every mid-edit keystroke. It trades a silent ceiling for confidently-wrong
  positions.
- **Best-effort partial types** need no new machinery (the map exists), are never
  stale (computed from the current rewritten buffer), and are positionally exact
  (real spans from this analysis).

**The mode boundary is preserved.** Only `Mode::Analyse` (the LSP) relaxes.
`Mode::Build` stays Ok-only: emission still requires a fully-typed file and still
bails on the first error, so **codegen correctness is untouched**. This keeps ADR
0093 D4's ceiling *boundary* intact — the value-receiver cell still owns the
overlay path and is the only cell that depends on analysis — while replacing its
all-or-nothing behaviour with a best-effort floor.

## Consequences

**Monotonic — never worsens.** The partial map is a superset of the clean map.
Clean files: byte-identical results. Dirty files: gain types for their well-typed
sub-expressions; an ill-typed receiver still yields nothing (conservative, as
today). Completion and signature help strictly improve; nothing regresses.

**Best-effort caveat.** A partial type may reflect error-recovery inference — a
receiver typed under an upstream error can be approximate. This is acceptable:
completion is a non-binding hint, the rewrite already runs on a recovery parse,
and the failure mode is "occasionally an approximate suggestion in an
already-broken file," never a wrong emission (Build never sees this map).

**Shape of the change (slice 4).** `check_record` must surface its partial
`expr_types` on the error path — e.g. return them alongside the errors rather
than dropping them — and the `Mode::Analyse` recorder in `project.rs` must commit
them past the `Err` branch. Build-mode callers ignore the partial map. Both
`type_receiver` consumers — value-member completion and signature help — gain
error tolerance with no change of their own.

**Applies at every check-phase exit.** A file's types are discarded at three
points, not one: `check_record` (top-level `fn`/method bodies), and — for
contexts/adapters — `check_context_constraints` and `check_context_declarations`,
the latter being where **service/agent handler bodies** are typed (the prime spot
for `.`-completion). All three record best-effort types in Analyse mode.

**Out of scope — the resolve gate.** Upstream of checking, `resolve_file_record`
validates the whole file; on a resolution error (an unresolved name *anywhere*)
the per-file loop bails before the checker runs, so there are no `expr_types` to
surface. Lifting that too would mean running the checker on an unresolved model
and discarding its (now-spurious) errors — more surgery and a diagnostics-noise
risk — so it stays a known remaining ceiling, not part of this slice. The common
mid-edit case (a *type* error elsewhere while the receiver itself resolves) is
covered; an *unresolved name* elsewhere still blanks the receiver.

**Out of scope — locals.** In-scope locals carry their own `ty: String` from the
`LocalsSink` (independent of `expr_types`) and are served from the cached round,
so they are largely error-tolerant already; their cache-positioning under errors
is a separate concern, not this ceiling.
