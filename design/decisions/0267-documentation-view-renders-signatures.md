# 0267 — Each declaration renders its signature, reusing hover's assembly

- **Status:** Accepted (v0.225)

**Context.** A page of doc prose alone is a comment dump; a *reference* shows
each declaration's signature too. And the signature+doc rendering already exists
— hover's `describe_*` in `bynk_ide::symbols`. Re-implementing it would risk the
two drifting (the proposal's own "divergence from hover" risk).

**Decision.** Each entry's Markdown — a fenced `bynk` signature followed by its
doc-comment prose — is produced by **hover's `describe_*` assembly**, made
`pub(crate)` and called from the new `documentation` module rather than copied.
The one gap it fills is `describe_service_handler`: a service handler has no
compound index key (its route, not a dispatch name, identifies it), so hover
never described one individually; the doc page is the first surface that renders
each, and it does so through the same fenced-signature + doc-prose shape every
other `describe_*` uses.

**Consequences.** The page cannot drift from hover — they share the renderer. A
new doc-bearing declaration kind is rendered by the same function hover already
uses for it.
