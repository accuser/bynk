# §9 Diagnostics

The `bynk.*` diagnostic codes are the **identifiers of the static-semantics
rules** ([§5](static-semantics.md)). Each well-formedness rule is named by the
code a conforming implementation emits when the rule is violated: when an
implementation rejects an ill-formed program, it MUST emit the code this
specification associates with the violated rule
([§1.3](scope.md), [§5](static-semantics.md)). The codes are stable identifiers
and are cited as such throughout §5.

The **complete catalogue** — every code, with a one-line summary and the
construct it governs, grouped by category — is the generated
[diagnostic index](../reference/diagnostics.md). It is produced from the
compiler's diagnostic registry and drift-tested, so it cannot diverge from the
codes the compiler actually emits; this chapter does not restate it.

> [!NOTE]
> The catalogue is currently shared with the friendly reference, which is its
> present canonical home, exactly as the grammar appendix is
> ([§11](grammar-appendix.md)). At the 1.0 reference-from-spec flip its canonical
> home may move under this specification. Either way it remains a single
> generated artefact. This note is informative.
