---
level: patch
changelog: "`bynkc fmt` / `bynk fmt` read the project's `bynk.toml` `[fmt]` section as the layer under their flags"
---

## ADR: fmt-config-layering
title: Formatting options resolve as defaults, then `bynk.toml` `[fmt]`, then flags
summary: Where a `fmt` run's options come from, and which component reads `[fmt]`

**Context.** `bynk.toml`'s `[fmt]` section was read by the language server
alone. Format-on-save honoured it; `bynkc fmt` and `bynk fmt` did not, so a
project that configured a style got different bytes from the editor and the
command line, and `fmt --check` in CI gated on a style the editor never
produced. The obvious fix — teach the CLI to read `[fmt]` — has a trap in it:
implemented directly, it puts a *second* parser of one section into the tree,
and the two then drift on what a key means, which is the defect being closed
rather than a fix for it.

A second question rides along. Once there is more than one source of an option,
their precedence has to be stated, and the CLI's argument parser has to be able
to tell "the user asked for the default value" from "the user said nothing" —
otherwise a flag nobody passed silently overrides a manifest that did.

**Decision.** Options resolve in three layers, each overriding the one before:
the spec defaults, then the project's `[fmt]`, then the flags the run passed. A
flag overrides only the field it names; a field no layer above mentions keeps
the value from below. `--no-config` drops the middle layer for a run that wants
the canonical style whatever project it is pointed at.

The `[fmt]` reader is `bynk_fmt::FmtConfig`, in the crate that owns
`FormatOptions` and that both front-ends already depend on. The language server
calls it too, so there is one parser, not one per front-end. Reading yields a
struct of `Option`s rather than a filled-in `FormatOptions`, which is what lets
an absent key mean *defer* instead of *reset to default*; the CLI's own
arguments are `Option` for the same reason, and deliberately carry no clap
`default_value`.

The manifest is resolved **per input file** — the nearest `bynk.toml` at or
above it, after absolutising the path — not once per run. A run may span two
projects, and one set of run-wide options would have to be wrong about one of
them. Configuration is nonetheless a whole-run precondition: every input's
options are resolved before any file is written, so a manifest error found on
the third input cannot land after the first two have been rewritten.

An unrecognised key in `[fmt]` is an error rather than being ignored.

**Consequences.** A project that sets `[fmt]` now formats to that style from the
command line, where it previously got the canonical one — a behaviour change for
any such project, and the point of the increment. `--check` gates on the
resolved options, so such a project has a CI gate that can pass.

`bynk-fmt` gains `toml` and `serde`. It stays off the compiler, which is the
property the crate is built around, but it is no longer dependency-free for a
third party that wants only the AST walk.

Refusing an unknown `[fmt]` key trades forward compatibility for catching the
typo: a manifest naming a key an older binary lacks is refused rather than
half-applied. That is the right side of the trade while the language is pre-1.0
and the manifest is small; a `max_line_length = 120` sitting in a project for
months while the formatter quietly used 100 is precisely what a configuration
layer must not do.

The two front-ends agree on any manifest that parses, and only then. The CLI
reports a malformed section and formats nothing; the language server falls back
to the canonical style and keeps serving, because it cannot refuse to run. A
typo therefore shows as an editor that stops honouring the section and a
`bynkc fmt` that fails loudly — the documentation says so rather than claiming
the two can never disagree.
