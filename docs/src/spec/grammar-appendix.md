# §11 Complete grammar

The **complete, verbatim grammar** — every production in one block, together with
the token-and-trivia summary — is the authoritative definition of Bynk's syntax.
It is generated from the `tree-sitter-bynk` grammar, so it cannot drift from the
parser. The per-construct productions embedded throughout §3 and §4 are drawn
from this same source.

The complete grammar is the
[grammar appendix](../reference/grammar-appendix.md). Where a production shown in
§3 or §4 and the appendix appear to differ, the appendix governs.

> [!NOTE]
> The appendix is currently shared with the friendly reference, which is its
> present canonical home. At the 1.0 reference-from-spec flip its canonical home
> may move under this specification. Either way it remains a single generated
> artefact. This note is informative.
