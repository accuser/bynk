---
level: patch
changelog: Project-level `check`/`compile` diagnostics (consumes cycles, path/name mismatches, the reserved-namespace and adapter-binding checks, `uses`/`consumes`/`exports` validation, provider signature matching, …) now render with ariadne source context in directory mode instead of the plain `[category] message` fallback
---

## ADR: attribute-project-level-diagnostics
title: Project-level validation diagnostics are attributed to their owning file
summary: Why directory-mode `check`/`compile` errors rendered without source context, and the site-by-site attribution that restores it

**Context.** In directory/project mode, `bynkc check` / `bynk check` (and
`compile`) route through `bynk_emit::project::compile_project`, which returns a
`ProjectFailure` carrying each diagnostic as an `AttributedError` — a
`CompileError` plus the project-relative `source_path` of the file it belongs to
— alongside a per-file `snapshots` map of the analysed text (ADR 0052). The
front-end renderer `bynk_driver::print_project_failure` renders ariadne source
context only when an error's `source_path` is `Some(_)` **and** present in
`snapshots`; every other error falls to a plain `[category] message` line with
no boxed report, no caret, no source excerpt, and any secondary labels dropped.
Single-file mode was unaffected — it always has one file to render against.

The render layer was correct. The gap was upstream: ~40 collection sites in
`bynk-emit/src/project.rs` pushed diagnostics **unattributed** (`push_for(None,
…)` / `extend_for(None, …)`) even though the `CompileError` already carried a
real span and the owning `ParsedFile` was in scope — usually as the `parsed[i]`
loop variable. So structurally-precise diagnostics (`consumes_cycle`,
`inconsistent_commons_name`, `kind_conflict`, `namespace.reserved`,
`adapter.no_binding`, the `uses`/`consumes`/`exports` family, provider signature
matching, and more) rendered bare. No test pinned the rich rendering, because
`expected_error.txt` fixtures assert category strings only (ADR 0198) — which is
why the regression went unnoticed.

**Decision.** Thread the owning file through every *attributable* `None` site,
attributing each error to `Some(&pf.identity_path)` — the key `snapshots` uses —
so `print_project_failure` takes the rich branch with no change to the render
layer. This is done site-by-site, not by a blanket replace, because not every
`None` has a single owning file:

- In-loop sites (the majority) attribute to `parsed[i].identity_path` /
  `pf.identity_path` directly.
- Helpers that returned a flat `Vec<CompileError>` now pair each error with its
  file: the consistency checks (`consistency.rs`) and `build_unit_table`
  (`symbols.rs`) return the primary-span file's `identity_path`; the
  `uses_span_of` / `parsed_alias_span` / `consumes_span_of` locators return the
  owning `parsed` index; `detect_consumes_cycles` (`graph.rs`) takes a per-unit
  "representative `consumes` clause" map so a detected cycle anchors on a real
  clause span in a real file rather than the `0..0` project-level fallback.
- `phase_validate_providers` recovers each provider's declaring file from the
  group's files, since the merged `UnitTable` has flattened them away.

Attribution exposes a diagnostic's **secondary** labels, which the plain
fallback never rendered. Some point into a *different* file than the primary
span. Two cases are handled distinctly. A diagnostic that is **always**
cross-file — `kind_conflict` and `inconsistent_commons_name` (both compare two
files) — carries its "first declared here" provenance as a **note** naming the
other file, never a label, so it renders correctly regardless of offsets. A
diagnostic that is **sometimes** cross-file — the `duplicate_*` / `exports`
labels, whose earlier declaration can sit in a sibling file of a multi-file unit
— relies on the renderer's demotion: `CompileError::report_for` now demotes a
label that is out-of-bounds **or** not on a char boundary of the rendered source
(the latter closes an ariadne byte→char panic on non-ASCII source, the #716
class). That guard is conservative: it cannot catch a cross-file span that
happens to be in-bounds *and* boundary-aligned, so such a label can still
underline the wrong file. Eliminating that residue needs per-label file identity
in `CompileError` — a follow-up beyond this fix; the always-cross-file cases,
which are the common ones, are already correct.

**Consequences.** Directory-mode project diagnostics now render with the same
boxed, source-underlined ariadne report a parse or resolve error already gets,
across ~40 categories. The flattened `compile_project` contract (which drops
attribution) is unchanged, so the `expected_error.txt` fixture suite is
unaffected; new in-crate tests pin the rich rendering (a `bynk-driver` render
test over a real `consumes` cycle, plus `graph.rs` unit coverage of the
span/attribution), since fixtures structurally cannot.

Three surfaces are deliberately left unattributed for now and render plain:
test-suite and integration-suite diagnostics (attributing them means threading a
file through `process_tests` / `process_integration_tests`' many internal push
sites — a separable follow-up), platform-lock enforcement (a cross-cutting
deployment check), and the embedded first-party/synthetic toolchain sources,
which are excluded from `snapshots` by design. Genuinely file-less diagnostics —
directory-discovery failures, the empty-project error, and file-vs-directory
conflicts — correctly stay unattributed. (An earlier claim that
`provider.dependency_cycle` was unattributed did not hold: it already flows
through the file-attributed per-context declaration check.)
