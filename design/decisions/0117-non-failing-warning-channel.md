# 0117 — a non-failing warning channel: a diagnostic's severity decides whether it fails the build; warnings surface but compile succeeds

- **Status:** Accepted (diagnostics infrastructure; 2026-06-25)
- **Realises:** the long-standing `Severity::Warning` that was display-only — making it load-bearing in the build path. Unblocks the query-algebra track's `bynk.list` deprecation (slice 1c / ADR 0116 D6) and its `@indexed` hygiene warnings (track Q7 / slice 3).
- **Relates:** ADR 0052 (LSP project diagnostics — the `diagnose` path already returns every diagnostic with its severity, unchanged here); ADR 0054 (structured suggestions — a warning may carry a machine-applicable fix, the deprecation's auto-rewrite); ADR 0116 D6 (the `bynk.list`→methods deprecation this enables); the query-algebra track Q12 (which surfaced the gap).

## Context

Bynk's diagnostic type already carries a `Severity` (`Error`/`Warning`), and
`Severity::for_error(category)` classifies two categories as warnings —
`bynk.parse.orphan_doc_block` and `bynk.given.unused_capability`. But severity
is **display-only**: it picks the word "warning"/"error" in rendering and maps
to the LSP severity. It does **not** decide whether a build fails. Every
`CompileError` the checker pushes — warning-category or not — lands in one
`errors` vec, and `compile`/`compile_project` return `Err` if that vec is
non-empty. The two "warnings" are even matched in **negative** fixtures (42, 66),
i.e. they fail compilation by design today (the checker comment says as much:
"emit as a warning-category error so the test harness can match it").

This blocks any **deprecation**. ADR 0116 D6 wants the `bynk.list` free functions
deprecated — warned during a transition window, then removed. Under the current
model a deprecation diagnostic would *fail every caller's build* — a removal, not
a deprecation. The same gap blocks §11's `@indexed` hygiene, which §11 specifies
as **warnings** (missing/unused/ambiguous index), not errors (track Q7).

The fix is a true warning channel: a warning surfaces but does not fail the build.

## Decisions

**D1 — Severity decides build outcome.** A diagnostic whose
`Severity::for_error(category)` is `Warning` is a **non-failing warning**: it is
reported, but its presence alone does not make `compile`/`check`/`compile_project`
fail. A `Severity::Error` diagnostic fails the build as today. Severity, derived
from the category, becomes load-bearing rather than cosmetic.

**D2 — Partition once at the boundary, not at call sites.** Checker, resolver,
and parser code keep pushing every diagnostic into the **same** sink — no call
site chooses a channel. The split happens once, at the `check`/`compile`
boundary: the accumulated diagnostics are partitioned by severity into
**errors** and **warnings**. This keeps the change to one place and means a
category's severity is the single source of truth (flip a category in
`for_error` and its diagnostics change channel everywhere).

**D3 — The build API carries warnings on success.** `compile` and
`compile_project` return their output **plus the warnings** on success, and
errors-plus-warnings on failure:

- `compile -> Result<Compiled, Vec<CompileError>>` where
  `Compiled { ts: String, warnings: Vec<CompileError> }`;
- `compile_project -> Result<ProjectOutput, ProjectFailure>` with
  `ProjectOutput.warnings` populated, and `ProjectFailure` also carrying any
  warnings alongside its errors (so a failed build still surfaces them).

The LSP `diagnose` path is **unchanged** — it already returns every diagnostic
with severity attached; only the *build* paths gain the partition.

**D4 — CLI: warnings print, exit 0; errors fail as before.** `bynkc compile` /
`bynkc check` with only warnings **succeed** — output is written, the warnings
print to stderr rendered with the "warning" word (`bynk_render::severity_word`,
already severity-aware). Any error still fails the command with a non-zero exit.
A `--deny-warnings` / `-Werror` flag that promotes warnings to failures is a
**named follow-on**, not v1 — the default is that warnings do not gate CI.

**D5 — The two existing warning-category errors become true warnings.** With D1,
`bynk.parse.orphan_doc_block` and `bynk.given.unused_capability` stop failing the
build — which is what "warning" always meant. Their negative fixtures (42, 66)
**move to positive** with an `expected_warnings.txt` assertion (D6): the program
now *compiles*, emitting the warning. This is the one behaviour change the channel
implies, made explicit rather than incidental.

**D6 — Test-harness surface: `expected_warnings.txt`.** A positive fixture may
carry an `expected_warnings.txt` (category + message substrings, exactly like
`expected_error.txt`) asserting the warnings emitted on an otherwise **successful**
compile. Negative fixtures stay error-only (a build that must fail). A positive
fixture with no `expected_warnings.txt` asserts **no** warnings, so the channel
cannot silently start warning on existing clean fixtures.

## Consequences

- **Checker / build boundary.** One partition step where `check`/`compile`/
  `compile_project` currently return their error vec; the `Compiled`/
  `ProjectOutput`/`ProjectFailure` shapes gain a warnings field (D3). Call sites
  that push diagnostics are untouched (D2).
- **CLI.** `run_compile`/`run_check` render warnings and key the exit code on the
  presence of *errors*, not all diagnostics (D4).
- **Tests.** Fixtures 42 and 66 migrate (D5); the e2e harness learns
  `expected_warnings.txt` and asserts the no-warning default on positives (D6).
- **Unblocks.** The `bynk.list` deprecation (query slice 1c) becomes a real
  warning + machine-applicable auto-fix (ADR 0054); `@indexed` hygiene (query
  slice 3 / Q7) lands as warnings per §11. Both were waiting on this.
- **LSP unaffected.** `diagnose` already surfaces warnings with the right
  severity; editors already render them as warnings (no build to fail).

## Alternatives considered

- **A separate warning sink threaded through every call site.** Rejected (D2):
  every `errors.push` would have to choose a channel — invasive and error-prone;
  the category already encodes severity, so one boundary partition suffices.
- **Keep the status quo (warnings fail).** Rejected: it makes "warning" a lie and
  structurally blocks deprecations and §11's index hygiene.
- **`-Werror` by default.** Rejected (D4): a warning that fails the build is an
  error; the value of the channel is precisely a diagnostic that informs without
  gating. The opt-in strict flag is a follow-on.
- **Leave `orphan_doc_block`/`unused_capability` as failing "warnings".**
  Rejected (D5): two diagnostics that render "warning" but fail the build is the
  exact confusion this ADR removes; they become the first real warnings.
