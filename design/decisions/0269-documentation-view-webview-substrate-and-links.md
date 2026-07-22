# 0269 — Shared webview substrate, HTML-disabled Markdown, http(s) link allow-list

- **Status:** Accepted (v0.225)

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
