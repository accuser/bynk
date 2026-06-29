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

const define = {
  __APP_ORIGIN__: JSON.stringify(appOrigin),
  __SANDBOX_ORIGIN__: JSON.stringify(sandboxOrigin),
};
const common = {
  bundle: true,
  outdir: "dist",
  target: "es2022",
  sourcemap: true,
  minify: process.env.BYNK_MINIFY === "1",
  define,
};

// The app runs on its own real origin → an ES module loads fine.
await esbuild.build({ ...common, entryPoints: { app: "src/app.ts" }, format: "esm" });

// The sandbox runs in a `sandbox="allow-scripts"` iframe with an **opaque ("null")
// origin**. A `type="module"` script there is fetched in CORS mode and a null origin
// can't load it; a **classic IIFE script is exempt** (no-cors), so the opaque-origin
// isolation is kept without serving CORS headers. Dynamic `import()` of the blob-URL
// graph + the module Worker still work (blob: is local, not CORS-gated).
await esbuild.build({ ...common, entryPoints: { sandbox: "src/sandbox.ts" }, format: "iife" });

// Static assets + the wasm module (fetched at runtime from the deploy root).
await cp("index.html", "dist/index.html");
await cp("sandbox.html", "dist/sandbox.html");
await cp("src/vendor/bynk_wasm_bg.wasm", "dist/bynk_wasm_bg.wasm");

console.log(`built dist/  (app=${appOrigin}  sandbox=${sandboxOrigin})`);
