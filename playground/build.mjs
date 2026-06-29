// Build the playground into dist/ with esbuild (in-browser track, slice 4).
//
// Two entry points — the app and the sandbox execution document — bundled as ES
// modules, plus the static assets (HTML, the wasm compiler glue + module). The app
// and sandbox origins are injected via `define`; defaults are the production hosts,
// overridden by BYNK_APP_ORIGIN / BYNK_SANDBOX_ORIGIN for local verification.
import * as esbuild from "esbuild";
import { cp, mkdir, rm } from "node:fs/promises";

const appOrigin = process.env.BYNK_APP_ORIGIN ?? "https://playground.bynk-lang.org";
const sandboxOrigin = process.env.BYNK_SANDBOX_ORIGIN ?? "https://sandbox.bynk-lang.org";

await rm("dist", { recursive: true, force: true });
await mkdir("dist", { recursive: true });

await esbuild.build({
  entryPoints: { app: "src/app.ts", sandbox: "src/sandbox.ts" },
  outdir: "dist",
  bundle: true,
  format: "esm",
  target: "es2022",
  sourcemap: true,
  minify: process.env.BYNK_MINIFY === "1",
  define: {
    __APP_ORIGIN__: JSON.stringify(appOrigin),
    __SANDBOX_ORIGIN__: JSON.stringify(sandboxOrigin),
  },
});

// Static assets + the wasm module (fetched at runtime from the deploy root).
await cp("index.html", "dist/index.html");
await cp("sandbox.html", "dist/sandbox.html");
await cp("src/vendor/bynk_wasm_bg.wasm", "dist/bynk_wasm_bg.wasm");

console.log(`built dist/  (app=${appOrigin}  sandbox=${sandboxOrigin})`);
