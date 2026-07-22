---
level: minor
changelog: VS Code documentation view — a file's doc comments as a rendered reference page (`bynk/documentationModel` + webview)
---

Implements #847. A read-only `bynk/documentationModel` LSP request and a VS Code
webview that renders the current file's declarations as a rustdoc-style
reference page. Tooling only — no grammar / AST / checker-semantics / emitter /
runtime change; the query consumes the existing `documentation: Option<String>`
fields and the `document_symbols` traversal shape, and reuses hover's own
`describe_*` signature+doc assembly. Second consumer of the shared webview
substrate #846 introduced, which this change factors out of the sequence view.

## ADR: documentation-view-file-scoped
title: The documentation view is file-scoped in Tier 1
summary: A doc page aggregates one file's declarations, not a whole context

**Context.** A `context` is the natural documentation unit — it is the module,
possibly spanning several files. But the per-file `document_symbols` walk
already exists and is the trivial Tier-1 build; context-aggregation (merging
every file of a multi-file `context`) needs a new cross-file assembly with no
precedent for this query.

**Decision.** Ship **file-scoped**. `bynk/documentationModel` builds its model
from a single committed snapshot's text — exactly like `document_symbols` and
`bynk/sequenceModel` — with no cursor position (the page is the whole file, so
the params are a bare `textDocument`). `bynk_ide::documentation::documentation_model`
walks the parsed unit's items with an *exhaustive* match on `CommonsItem`, so a
new declaration kind is a compile error, not a silently-missed page row.

**Consequences.** A multi-file `context` documents one file at a time.
Context-aggregation is the deferred follow-up. A `suite` unit has no doc page
(its `case`/`stub` members have no `describe_*` renderer) and returns empty,
on the same terms as a non-project file or a file with no committed round.

## ADR: documentation-view-shows-undocumented
title: Undocumented declarations render as a coverage signal
summary: The page lists undocumented declarations with a placeholder, toggle to hide

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

## ADR: documentation-view-renders-signatures
title: Each declaration renders its signature, reusing hover's assembly
summary: The page is a reference (signature + doc), not a comment dump, sharing hover's renderer

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

## ADR: documentation-view-tier-1-on-demand
title: Documentation view Tier 1 — on-demand, not live-synced
summary: The page is built from the committed round on command, with no refresh push

**Context.** As with #846, two shapes were possible: a command that builds a
static page from the committed round (Tier 1), and a panel that follows the
active file and highlights on cursor (Tier 2).

**Decision.** Ship **Tier 1**. `bynk/documentationModel` is served from the
`committed_analysis` gate (the #733 stale-while-revalidate mechanism the
pull-based decoration handlers use) and is re-issued by the client each time the
"Bynk: Show Documentation" command fires. There is **no** refresh-push — no
generic "refresh a custom method" exists in the LSP spec or `tower_lsp::Client`,
and Tier 1 does not need one (same reasoning as `bynk/sequenceModel`).

**Consequences.** The page does not update while the user keeps editing with it
open; re-invoking the command gets a fresh render. Tier 2 (live follow +
cursor↔declaration highlighting) and context-aggregation are the deferred
follow-ups.

## ADR: documentation-view-webview-substrate-and-links
title: Shared webview substrate, HTML-disabled Markdown, http(s) link allow-list
summary: The doc view reuses #846's webview substrate and renders doc Markdown safely

**Context.** #846 built the extension's first webview inline in
`sequenceDiagram.ts`: the CSP + per-render nonce, the payload-embedding HTML
shell, and `postMessage`→reveal click-to-code. This view is the second consumer.
It also renders **untrusted-ish Markdown** — a doc comment could carry raw HTML,
a `<script>`, or an external link — inside a webview.

**Decision.** Factor the host-side substrate into a shared `webviewHost.ts` that
both webviews consume (issue #847's "built once, two consumers"); the only
per-view delta is the vendored renderer bundle (Mermaid for #846, `markdown-it`
here, each its own esbuild target). Render doc Markdown with **`html: false`**
(raw HTML in a doc comment becomes text, never markup) and **`linkify: false`**
(a *bare* URL stays literal — an explicit `[text](url)` or a `<url>` autolink
still renders as an anchor), under the same `default-src 'none'` CSP. Whatever
renders as a link, a click is gated through an **http(s) allow-list** — the
webview posts an `openExternal` message only for an `http(s)` href, and the host
re-checks the scheme before `vscode.env.openExternal`
(which shows its own trust prompt); a `command:`/`file:`/`vscode:` href is inert.
Unlike #846's SVG-order zip, the doc DOM is built element-by-element, so each
heading/signature holds its own click-to-code span directly.

**Consequences.** A doc comment cannot execute script, load an external
resource (CSP), or drive the host to a non-http(s) URI. The two webviews share
one CSP/nonce/reveal implementation, so a fix to the substrate reaches both.
