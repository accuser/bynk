# Bynk Playground

The in-browser REPL for Bynk (in-browser track, slice 4 — [ADR 0140](../design/decisions/0140-repl-execution-and-sandbox.md)).
Type Bynk, press **Run**, see it execute — with **no install and no server**: the
compiler runs in the browser as wasm, and the compiled JavaScript runs in a sandbox.

A fully static, client-side app. It deploys to two origins:

- **`playground.bynk-lang.org`** — the app: the editor + the `bynk_compile` wasm.
- **`sandbox.bynk-lang.org`** — the execution document: a sandboxed `<iframe>`
  wrapping a Web Worker. Untrusted code runs **only** here (the safety boundary).

## Layout

| Path | What |
|---|---|
| `src/app.ts` | The app: editor (CodeMirror 6), compile, diagnostics, run, deep-link, Share, examples picker. |
| `src/examples.ts` | The examples gallery — curated, **runnable** in-process snippets (each verified to compile + run); the header picker loads them. |
| `src/sandbox.ts` | The execution document: links the JS graph to blob-URL modules, runs it in a Worker under a wall-clock timeout, posts results back. |
| `src/deeplink.ts` | The shared snippet format: `#base64url(deflate-raw(utf8(source)))`. |
| `src/highlight.ts` | CodeMirror Bynk highlighting (stream-based; see *Highlighting* below). |
| `src/shared.ts` | Origin config + the postMessage protocol. |
| `scripts/build-wasm.sh` | `cargo build --target wasm32 -p bynk-wasm` + `wasm-bindgen` → `src/vendor/`. |
| `scripts/build-grammar.sh` | `tree-sitter build --wasm` → the web-tree-sitter grammar (needs emcc/docker). |
| `build.mjs` / `serve.mjs` | esbuild build into `dist/` / a two-port local static server. |

## Build

```sh
cd playground
npm install
npm run build:wasm        # needs the wasm32 target + wasm-bindgen-cli (matching the crate)
npm run build             # esbuild → dist/  (production origins by default)
```

`build:wasm` defaults to a debug wasm (~18 MB, fast to build). For deploy use a small
artefact:

```sh
npm run build:wasm -- --release   # release + wasm-opt -Oz if available
```

## Verify locally

The app and sandbox must be **different origins**. Build for localhost and serve on
two ports:

```sh
BYNK_APP_ORIGIN=http://localhost:8080 BYNK_SANDBOX_ORIGIN=http://localhost:8081 node build.mjs
node serve.mjs        # app → http://localhost:8080 , sandbox → http://localhost:8081
```

Open `http://localhost:8080`, then check:

1. The starter program is shown; **Run** (or ⌘/Ctrl-Enter) logs a line and prints a value.
2. Break the program → an error diagnostic appears with a line:col, and it does not run.
3. A `consumes bynk.cloudflare { … }` program shows *not runnable in-browser* (the platform lock).
4. **Share** puts a `#…` link in the address bar; reloading that URL restores the source.
5. An infinite loop (`fn` that never returns) is terminated after the wall-clock budget.

> The core logic (wasm compile, the emitted graph running, the blob-URL linker, the
> deep-link round-trip) is verified in Node; the browser DOM/iframe/Worker flow is
> verified by the steps above.

## Share service (`share/`) — written in Bynk

Short share links (`?s=<id>`) are backed by a **Bynk program** (`playground/share/`)
compiled by `bynkc` to a Cloudflare Worker + KV — dogfooding: the playground that
compiles Bynk has a backend *written* in Bynk. `POST /api/snippets` stores the source
under a random id; `GET /api/snippets/:id` returns it. The `Source` refined type
bounds the body, so oversized/empty payloads are rejected at the boundary (`400`).

The browser calls it **same-origin** (`/api/*` on the app origin) — Bynk's `from http`
emits no CORS headers, so cross-origin would not work; same-origin routing avoids CORS
entirely. If the service is unavailable, Share falls back to the self-contained
`#hash` link, so the playground works without it.

Build + run + verify locally:

```sh
# Compile the Bynk service to a JS Worker:
bynkc compile share --target workers --platform cloudflare --emit js -o /tmp/share-js
# Run it (set wrangler.toml main=index.js + a dummy local KV id):
cd /tmp/share-js/workers/snippets && npx wrangler@4 dev --port 8799 --local
# serve.mjs proxies /api/* → http://localhost:8799 (override with BYNK_SHARE_WORKER),
# so the app at :8080 reaches it same-origin.
curl -X POST :8080/api/snippets -d '{"source":"context x.y\n"}'   # → {"id":"…"}
```

## Deploy (Cloudflare Pages — maintainer ops)

Two Pages projects, both serving `dist/` built with the production origins (the
default), plus the two DNS records:

- `playground.bynk-lang.org` → the app project.
- `sandbox.bynk-lang.org` → the same `dist/` (it serves `sandbox.html` + `sandbox.js`).

Until they serve, links degrade to "coming soon" (the documentation track's plan).

For short share links, deploy the `share/` Bynk Worker (compile with `--emit js`, then
`wrangler deploy` with a real KV namespace bound as `KV`) and add a Cloudflare route so
`playground.bynk-lang.org/api/*` reaches it — same-origin, so no CORS. Optional: the
app falls back to `#hash` links without it.

## Highlighting

Highlighting is **web-tree-sitter** (in-browser track Q4): `tree-sitter-bynk` compiled
to wasm by `scripts/build-grammar.sh`, parsed and queried (`queries/highlights.scm`)
in `src/tshighlight.ts`, with captures rendered as CodeMirror decorations — the same
grammar the editor and CLI use. The grammar wasm build needs **`emcc` or a running
docker daemon**; run `npm run build:grammar` to produce `src/vendor/{tree-sitter,
tree-sitter-bynk}.wasm`.

The editor starts on the lightweight CodeMirror **stream highlighter**
(`src/highlight.ts`) and **swaps to web-tree-sitter once its wasm loads** (via a
Compartment). If the grammar wasm is absent or fails to load, the stream highlighter
stays — so the build (and the deploy) degrade gracefully without it.
