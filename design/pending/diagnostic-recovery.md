---
level: minor
changelog: Diagnostics recover past an error instead of stopping at it. An unknown name in one declaration no longer hides type errors in the others, including service and agent handler bodies. `bynkc check` reports every syntax error in a file, not just the first, and checks the declarations that did parse. A declaration that fails to parse no longer cascades into a wave of unknown-name and unknown-type errors at every reference to it (one missing comma in a record went from 21 diagnostics to 1). A pattern or `let` that fails to check still binds its names, so their uses do not echo as unknown names. Nothing with an error is ever emitted (#1663)
---

## ADR: diagnostic-recovery
title: Check per declaration, and treat a broken declaration's name as known
summary: Resolve-then-check runs per declaration rather than per unit, a declaration the parser skips registers its name, and failed bindings are error-typed, so one fault yields one diagnostic

**Context.** Four recovery gaps (#1663, the 2026-10-01 review, Part 4) each
turned one fault into a hidden or multiplied set of diagnostics:
1. **A resolve error hid the unit.** Any resolve error returned before the
   checker ran, on the CLI and editor paths alike, so an unknown name in `b`
   hid a type error in `a`.
2. **The CLI stopped at the first syntax error.** Its parse was the strict,
   single-error one; only the editor used the recovering parser.
3. **A broken declaration cascaded.** Recovery skips a declaration that fails
   to parse, and its name went with it: one missing comma in `Money` yielded
   20 `unknown_type`/`method_unknown_type` echoes.
4. **A failed pattern dropped its names.** After `pattern_arity` or
   `unknown_pattern_field`, the arm's bindings were unbound, so each use was a
   spurious `unknown_name`.

**Decision.**
- **A: per declaration (recommended in the issue).**
  - The resolver returns its table *with* its errors (`resolve_recovering`),
    and the checker runs on every declaration.
  - Within a declaration the resolver rejected, the checker's diagnostics are
    its echoes of the same fault (`unknown_function` twice, the checker's
    `types.unknown_static_member` twin, `unknown_name` for a misplaced `self`),
    so they are dropped (`without_resolve_echoes`). Every other declaration
    keeps its type errors.
  - On the project path the later stages run past a checker error too, so a
    service handler's body is still typed when a free function failed. Each
    file keeps only the later-stage diagnostics located in it: those stages
    walk the whole unit and used to attribute another file's fault to the
    current one, at a position in the wrong source.
- **B: broken declarations are known names (recommended in the issue).**
  - The parser records the name of each top-level declaration recovery skips
    (`Recovered::broken_decl_names`).
  - An unknown-name diagnostic naming one is hidden, because the declaration's
    own syntax error already reports the fault (`split_broken_decl_echoes`).
  - It still counts as a resolve error for A, so the checker's follow-ons in
    the referencing declaration are dropped too.
  - The names are matched in the diagnostic's message, scoped to the five
    unknown-name codes. That is cheaper than threading a name set into a dozen
    emission sites.
- **Error-typed bindings.**
  - A pattern that fails to check, and a `let`/`<-` whose value fails to
    type, bind their names to `Ty::Error`.
  - A use of an error-typed name types as unknown (`None`), which every
    enclosing check already absorbs.
- **The CLI recovers.**
  - The strict parse still runs first, so a clean file is unchanged.
  - When it fails, a recovering parse reports every syntax error, merged with
    the strict one, which always survives: some rules hold only for the strict
    single-unit parse.
  - Single-file `bynkc check` then resolves and checks the declarations that
    parsed, under A and B.
  - On the project path a file that fails to parse still drops out of
    checking; its syntax errors are now all reported.
- **Recovery skips a rejected item whole.** An item keyword that is illegal at
  this position (`agent` in a commons) is skipped together with its name and
  body, instead of one token. The `}` closing the item loop's own body is
  never consumed by the no-progress step. Each of those had produced a
  follow-on syntax error.

**Consequences.**
- Emission is unchanged: any error still refuses `compile`, `compile_project`
  and `bynkc test`. `recovery_never_emits.rs` pins all three.
- Across the negative fixture corpus, the only new diagnostics are those the
  `1663_*` fixtures intend. About 25 `unknown_name` echoes of failed lets and
  patterns are gone.
- An LSP test pinned the old behaviour where an unrelated resolve error blanked
  a file's types for completion (the gap ADR 0094 left open). It now asserts
  the typed completions survive.
- The negative-fixture harness had only subset checks, so it could not see a
  spurious diagnostic. `expected_error.txt` gains an exact-set mode (`# exact`):
  needles and diagnostics must pair one to one.
- Not in scope: on the project path, checking a partially-parsed file's
  surviving declarations (and suppressing references to its broken names from
  other files).

Proved by:
- the exact-set fixtures `1663_resolve_error_keeps_type_errors` (single-file
  and `_project`), `1663_syntax_errors_all_reported`,
  `1663_broken_record_no_cascade` and `1663_pattern_arity_keeps_bindings`,
  each first pinned at the old output and then flipped by its fix;
- `bynk-ide/tests/recovery.rs` (the editor path: the 21-to-1 `Money` case, and
  broken functions and methods);
- a corpus diff of every negative fixture against `main`;
- 4,600 mutated positive fixtures through `bynkc check` across three sweeps,
  with no panic or hang this change introduced (the one panic found also
  occurs on `main`, and is #1708).
