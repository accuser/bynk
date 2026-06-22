# Debug in VS Code

**Goal:** set a breakpoint in a `.bynk` file, press **Debug**, and step through
your code as it runs — under Node (tests) or `workerd` (the dev server).

The `vscode-bynk` extension delegates to VS Code's built-in JavaScript debugger:
it compiles your project, starts the V8 inspector, and attaches automatically.
Breakpoints set in `.bynk` sources resolve through the emitted source maps to the
exact statement, so you debug *Bynk*, not the generated TypeScript.

## Prerequisites

- The `vscode-bynk` extension installed (see [Set up editor
  support](editor-support.md)).
- `bynkc` and `bynk` on your `PATH` (or set `bynk.compilerPath` / `bynk.bynkPath`).
- **Node ≥ 22.6** for the test path (the emitted `.ts` runs under Node's
  type-stripping). For the dev-server path, the same toolchain
  [`bynk dev`](../../reference/bynk-cli.md#bynk-dev) needs (`wrangler`).

## Debug a test

1. Open a `.bynk` test file and click in the gutter to set a breakpoint — on a
   test-body statement, or in the production code the test exercises.
2. Open the **Testing** view (the flask icon). Hover a test or suite and click
   the **Debug** action (beside Run), or run the **Debug** profile from the
   Test Explorer.
3. The extension shells `bynkc test --inspect`, attaches, and execution pauses at
   your breakpoint. Step, inspect variables, and continue as usual.

The whole suite runs under the debugger; your breakpoint decides where it stops.
The Bynk runtime and generated glue are skip-stepped, so stepping stays inside
your code.

## Debug the dev server

1. Set a breakpoint in a `.bynk` handler (e.g. a `service`/`agent` request
   handler).
2. Create a `launch.json` (Run → Add Configuration → **Bynk**) — or use the
   snippet **Bynk: Debug dev server**:

   ```json
   {
     "type": "bynk",
     "request": "launch",
     "name": "Bynk: Debug dev server",
     "mode": "dev"
   }
   ```

3. Press **F5**. The extension runs `bynk dev --inspect`, attaches to
   `wrangler`'s inspector, and your handler **pauses on a real request** — send
   one (e.g. `curl http://127.0.0.1:8787/`) and step through it.

For a multi-context project, set `"context": "<name>"` to choose which worker to
serve and attach to. `"port"` overrides the inspector port (default `9229`).

## How it works

Per [ADR 0104](../../decisions/0104-debug-launch-model.md) this is *glue, not a
Debug Adapter*: the extension contributes a `bynk` debug type whose
configuration provider starts the inspector by shelling the
[`--inspect` CLIs](../../reference/bynk-cli.md) and hands off to VS Code's
JavaScript debugger (`pwa-node`). The source maps — `.bynk` files referenced by
absolute path so an editor breakpoint resolves to the same source the debugger
loads — do the breakpoint relocation. You can also run the CLIs directly and
attach any JavaScript debugger by hand; the one-click flow just automates that.

## Troubleshooting

- **The breakpoint shows as unbound (hollow).** Make sure the project is the
  open workspace folder — the debugger anchors source resolution there. Rebuild
  if the emitted output is stale.
- **`node` not found / wrong version.** The test path needs Node ≥ 22.6 for
  TypeScript type-stripping; check with `bynk doctor`.
- **Nothing happens on the dev path.** Confirm `wrangler` is available
  (`bynk doctor --only deploy`) and that a request actually reaches the handler.
