# 0146 — `suite`/`case` replace the overloaded `test` container

- **Status:** Accepted (testing track, slice 1a; v0.112).
- **Provenance:** the testing feature track's first slice (DECISION Q), the
  `suite`/`case` half. The track's vocabulary is `suite` / `case` / `property`;
  `property` (with `for all`) arrives in slice 2, so this ADR settles the container
  and case keywords only.
- **Realises:** the replacement of the doubly-overloaded `test` keyword with two
  distinct keywords — `suite` for the container and `case` for a named test — so a
  test file reads as a structure of named cases rather than nested `test`s.
- **Relates:** ADR 0145 (`expect` — the predicate a `case` body checks, landed in the
  same slice); ADR 0144 (one predicate surface — the testing track's organising
  commitment this slice first embodies); ADR 0098 (`--format json` — the runner's
  `suites`/`cases` document, whose `name`/`kind` shape this rename leaves intact).

## Context

The test container keyword was `test`, used at **two** nesting levels for **two**
different things: `test money { … }` declared a container targeting a unit, and
`test "a tenth off a tenner" { … }` inside it declared a single named test. One word
did the job of both "group of tests" and "a test", so the grammar had to disambiguate
by argument shape (an identifier vs a string) and a reader had to do the same. The
same overload extended to `test integration "checkout" { … }`.

The testing track's vocabulary distinguishes these roles by name: a **`suite`** is
the container (naming its target unit), a **`case`** is one named test inside it. This
is a straight rename with no change of meaning — the target of a suite is still an
identifier, a case is still a string — but it removes the same-keyword-two-levels
overload and makes the container/member relationship read directly.

## Decision

**D1 — The container keyword becomes `suite`; the per-test keyword becomes `case`.**
`test <id> { … }` → `suite <id> { … }`; the inner `test "…" { … }` → `case "…" { …
}`. Both are new keywords; `test` is removed, not deprecated (clean-slate posture).
Nothing about targeting or nesting moves — a `suite` still names its target unit by
identifier and a `case` is still a string-named block.

**D2 — `suite integration` is renamed for uniformity but not otherwise reworked.**
`test integration "checkout" { wires …; test "…" { … } }` becomes
`suite integration "checkout" { wires …; case "…" { … } }`. The `integration` /
`wires` machinery survives untouched — its rework into the tier dial (`as unit |
integration | system`) is slice 6, which absorbs it. This slice only aligns the
spelling.

**D3 — `mocks` and `Mock[T]` are unchanged.** The `mocks` collaborator-substitution
block and the `Mock[T]` value fabricator keep their surface and their diagnostics in
this slice; they retire in later slices (`Mock[T]` → `Val[T]` in slice 2, `mocks` →
`provides` in slice 6). Only the container/case keywords move here.

**D4 — The diagnostic family is normalised onto `bynk.suite.*` (ADR 0144 D4).**
`bynk.test.duplicate_case_name`, `bynk.test.unknown_target`, and the rest →
`bynk.suite.*`; the parse diagnostic for a mis-started container becomes
`bynk.parse.unexpected_suite`. Category titles move with them (`"suite"` → "Suites
and cases"). The `bynk.test.*` codes are retired.

**D5 — The runner document keeps its shape.** The `--format json` output already
carried `suites` with a `kind` (`"unit"` / `"integration"`) and a clean `name` per
case (ADR 0098); the keyword rename changes the source spelling, not the runner's
JSON — so tooling (the VS Code Test Explorer's discovery tree) is unaffected.

**D6 — `property` is out of scope.** The track's third vocabulary word — `property`,
with `for all`, for generated inputs — is DECISION C/P territory and lands in slice 2
with `Val[T]`. This ADR settles `suite`/`case` only; `property` cites it plus its own
slice's ADR when it arrives.

## Consequences

- A test file reads as a container of named cases — the container/member relationship
  is in the keywords, not inferred from whether the argument is an identifier or a
  string; the grammar's two-level `test` overload is gone.
- The rename touches every layer (lexer/keywords, AST — `TestDecl` → `SuiteDecl`,
  `TestCase` → `Case`; parser, checker's `in_test_body` gate, formatter, tree-sitter
  grammar, the VS Code TextMate keyword list) but is shallow at each — no semantics
  move.
- `bynk.test.*` is gone from the registry, replaced by `bynk.suite.*`; the runner's
  JSON contract (ADR 0098) is preserved.
- `suite integration` is now spelled uniformly with unit suites, which is the seam
  the slice-6 tier dial folds into a single `suite` with an `as` tier clause; this
  rename does that folding no harm and some good (one container keyword to dial).
- `property` is the named, deferred third vocabulary word this ADR deliberately holds
  for slice 2.
