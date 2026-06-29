# 0140 — The REPL/playground: execution model, the cross-origin sandbox, and the deep-link read side

- **Status:** Accepted (in-browser track, slice 4; v0.108.4).
- **Provenance:** the closing slice of the in-browser track — the playground itself,
  tying together the JS artefact (ADR 0137), the Browser binding (ADR 0138), and the
  wasm toolchain (ADR 0139). **Security-bearing** (it executes other people's Bynk in
  the user's browser); `/security-review`-gated per the track posture.
- **Realises:** the design notes' §19 "a REPL is ambitious and probably v2 or v3" —
  delivered. Also **ratifies the read side** of the shared snippet/deep-link format
  (Q7), settled jointly with the documentation track (which emits the same links).

## Context

The playground lets a newcomer type Bynk and see it run with no install. It compiles
in the browser (ADR 0139's `bynk_compile` wasm) and must *run* the result — untrusted
code, including link-borne snippets that execute *other people's* Bynk. That is the
track's safety boundary. The app is fully static (client-side compile + eval), so it
hosts on Cloudflare Pages with no server compute.

## Decision

**A fully static two-origin app: the editor/compiler on the app origin, execution
isolated on a separate sandbox origin.**

- **D1 — Stack.** `esbuild` + vanilla TypeScript + CodeMirror 6, no framework — a
  single-page tool. It bundles two entry points (the app, the sandbox document) and
  the wasm compiler glue. Deploys as static assets to two Cloudflare Pages projects:
  `playground.bynk-lang.org` (app) and `sandbox.bynk-lang.org` (execution).

- **D2 — The execution sandbox (defence-in-depth).** The execution document is served
  from the **separate sandbox origin** and embedded by the app as
  `<iframe sandbox="allow-scripts">` (no `allow-same-origin` → a unique opaque
  origin), wrapping a **Web Worker**. So isolation is layered: a distinct hostname
  (nothing on it), an opaque sandbox origin, and a Worker. An infinite loop or runaway
  allocation is bounded by a **hard wall-clock budget** — the Worker is
  `terminate()`d on overrun and the UI reports a timeout. `Fetch` and `Secrets` are
  already withheld (they throw) in the Browser binding (ADR 0138), so the sandbox can
  reach neither the network nor secrets.

- **D3 — Linking.** `bynk_compile` returns the full JS module graph (user module +
  `runtime.js` + `bynk-browser.js` + `compose.js`). The sandbox links it into
  **blob-URL ES modules in topological order**, rewriting each module's relative
  import specifiers to its dependencies' blob URLs (Workers don't support import maps,
  so specifier rewriting is the portable approach). The Worker imports the entry,
  calls `composeApp()`, and invokes the single **zero-argument** service handler,
  capturing `Logger` output and the returned value.

- **D4 — Message trust.** App → sandbox iframe carries the compiled graph
  (`targetOrigin "*"`, since the sandbox origin is opaque); the **sandbox validates
  `event.origin === app origin`** before acting. Sandbox → app posts results to the
  app's real origin; the **app accepts only messages whose `source` is its sandbox
  iframe's window** (the sandbox origin being opaque). Only a structured-clone result
  channel crosses back — no DOM handle, no storage/cookie access to the app origin.

- **D5 — Deep-link (Q7 read side, ratified).** A shared snippet is the source
  **compressed into the URL fragment**: `#<base64url(deflate-raw(utf8(source)))>` via
  the browser-native Compression Streams API (no library dependency on either side of
  the contract). The REPL decodes it on load; the documentation track emits exactly
  this format. Granularity is **whole-unit** (Q5) with a starter template.

- **D6 — Subset.** A program that reaches Workers/Cloudflare-only shapes is rejected
  by the slice-2 platform lock (`bynk.target.vendor_required` /
  `browser_bundle_only`); the UI shows the reason as *not runnable in-browser* and
  does not run it — reusing the existing platform-lock reasoning, not a parallel one.

## Consequences

- The educational on-ramp the design notes always pointed at: type Bynk, press Run, in
  a browser, nothing installed. The compiler, the binding, and the sandbox are all
  client-side.
- **Deployment is an ops step the maintainer owns** — two Pages projects + the two DNS
  records (and the app/sandbox origins are build-time constants). The docs track's
  "coming soon" degradation covers the pre-serving window.
- **Size:** the debug wasm is ~18 MB; the deploy build uses `--release` + `wasm-opt`
  (the Q3 budget) and lazy-loads the module. Correct without it; just larger.
- **Highlighting:** the intended end-state is `tree-sitter-bynk` via web-tree-sitter
  (Q4); building that grammar wasm needs `emcc`/docker, so the MVP ships a faithful
  CodeMirror stream highlighter and `scripts/build-grammar.sh` stands ready — the
  web-tree-sitter swap is the named follow-on.
- **The in-browser track closes** with the playground; remaining items (a share-id
  persistence service, an examples gallery, LSP-in-browser) are the optional slice-5
  polish, cut once the base proves out.
