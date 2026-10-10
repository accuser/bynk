---
level: minor
changelog: "**Module docs** ([#1885](https://github.com/accuser/bynk/issues/1885)). A doc-block above a `commons`/`context`/`adapter` header is the unit's module doc. A unit split across files carries at most one: two or more is `bynk.project.duplicate_module_doc`, naming every file that carries one. Hover on a unit name (its header, a `uses` target, a `consumes` target) shows the unit's module doc, found in whichever of its files has it, and the documentation page merges every file of a multi-file unit under that doc. New guide: [Document your code](/book/guides/program-structure/document-your-code/); spec [§5.1a](/book/spec/static-semantics/#module-docs)."
---

## ADR: one-module-doc-per-unit
title: A unit split across files carries at most one module doc
summary: Two module docs on one unit are an error naming every file; hover and the documentation page read the one there is

**Context.** A doc-block above a unit header has been parsed and stored as the
unit's documentation (`Context.documentation`, `Commons.documentation`) since
doc-blocks existed, but nothing defined it. The book never mentioned it, hover
had no case for a unit name, and the documentation page was file-scoped, with
merging a multi-file context deferred. A context (and a commons) may be split
across files that share its header, and each file could carry its own doc, so
"the unit's doc" had no single meaning. #1885 found this in compat-board, whose
files open with `--` comments that are really module docs.

**Decision.**

- **D1: at most one module doc per unit.** When two or more files of a unit
  carry a module doc, each such file gets `bynk.project.duplicate_module_doc`
  at its header name. The message names every file that carries one, with
  paths `/`-joined on every platform. Alternatives rejected: concatenating in
  file order (the result depends on file names and reads oddly), and
  designating a file by name, such as one matching the unit (a convention
  dressed up as a rule).
- **D2: the rule is kind-agnostic.** The issue scoped it to contexts on the
  premise that a commons is one file, but a multi-file commons is legal (ADR
  0160), and the same ambiguity applies. The check runs over every production
  group of two or more files. A `suite` is grouped separately and never
  counted: its doc describes the suite, not its target.
- **D3: hover on a unit name shows the module doc.** A new lexical rung, tried
  before the bare-name rungs, answers when the cursor is on the file's own
  header name or a `uses`/`consumes`/suite target path, with `<kind> <name>`
  and the doc from whichever of the unit's files carries it (the project's
  other files, else the analysed round's snapshots, then the embedded
  first-party sources). It is guarded to those names' byte ranges, which hold
  only unit-path segments, so it cannot shadow the binding-index rung; running
  before the bare-name rungs stops a path segment (`model` in `uses
  compat.model`) from answering with a same-named declaration.
- **D4: the documentation page merges a multi-file unit.** With D1 the merge is
  unambiguous: the lede is the one module doc, and the entries are every file's
  declarations, files in path order. Each wire entry carries its own file's URI
  and a range lowered against that file, so click-to-code opens the right one.
  The page is the same whichever of the unit's files it is opened from.

**Consequences.** One existing negative fixture
(`100_context_rebrand_construction`) had a module doc in both of its files; the
second became a `--` comment. The doc-example gate now reads a block's unit
header past a leading doc-block or `--` comment, so a module-doc example
compiles as the unit it shows. The book gains a "Document your code" guide and
§5.1a.
