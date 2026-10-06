# 0435 — The project path checks a partially-parsed file's surviving declarations

- **Status:** Accepted (v0.306)

**Context.** #1663 (ADR 0433) made diagnostics recover past errors, but only
on the single-file paths. On the project path (`bynkc check <dir>`, the
editor's project analysis), a file that failed the strict parse dropped out of
checking entirely, and references to its names from other files echoed as
unknown. The project path parses through `bynk-project`'s parse cache, which
stored only the strict parse (ADR 0416, DECISION E). Its `ExprId`s come from
a durable counter, so a recovered AST parsed outside it would collide with
every other file's ids.

**Decision.**
- **Recovery is cached beside the strict failure.** When the strict parse
  fails in the parser, the cache entry also keeps the recovering parse of the
  same tokens, with ids from the same durable counter
  (`parse_units_recovering_from`, read through `cached_recovery`). This amends
  ADR 0416's DECISION E without weakening it: the strict result is unchanged
  and is still the `Err` the build reads, and the recovered units are for
  diagnostics only. A broken file keeps the same ids across analyses.
- **Recovered units join the project.** `phase_parse` adds a broken file's
  recovered units to the parsed set, so Decision A's per-declaration checking
  reaches its surviving declarations.
- **Skipped names are known, scoped to what can see them.** `phase_parse`
  records each unit's skipped declarations; a file's unit sees its own, and
  those of the units it `uses` and `consumes`. Unknown-name echoes of those
  are split out (Decision B) from everything past the resolver too: the
  checker reports a skipped declaration under its own codes (a consumed
  context's service, a method, a capability in a provider or `given`, an
  actor), so its errors and warnings are split on the project path and on
  the single-file paths alike, and provider validation skips a capability
  recovery dropped. A unit that can't see the broken one still gets its
  genuine unknown-name error.
- **A build still fails fast.** In build mode any error from the phases
  before checking stops the pipeline, and a syntax error is no exception; the
  check path (`bynkc check <dir>`, the editor) is where the full set is
  reported. Nothing with an error is emitted.

**Consequences.** Fixing a syntax error no longer reveals a second round of
type errors elsewhere in the project. A failing file's recovery is one extra
parse when its content changes, cached with the strict parse. The suppression
matches by declaration name within the visible units, so a genuine unknown name
that happens to match a skipped declaration of a visible unit is hidden until
the syntax error is fixed, the same trade #1663 made within one file.
