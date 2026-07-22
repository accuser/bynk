# 0266 — Undocumented declarations render as a coverage signal

- **Status:** Accepted (v0.225)

**Context.** A doc page could omit undocumented declarations for a clean read,
or show them with a "no documentation" note so the view doubles as a
doc-coverage report — the same dead-code-signal logic as the `"0 references"`
CodeLens.

**Decision.** **Show them.** Every declaration carries a `documented` flag; the
webview renders an undocumented declaration's signature followed by a
*No documentation* placeholder, and offers a toggle to hide the undocumented for
a clean reading page. The model is identical whether or not a declaration
carries docs — the flag is the only difference.

**Consequences.** The view is a doc-coverage report as well as a reference. The
toggle is a pure client-side filter (a `data-` attribute + CSS), so it needs no
re-request.
