// Local static server for verification (in-browser track, slice 4). Serves dist/
// on two ports so the app (8080) and the sandbox (8081) are genuinely
// cross-origin — exercising the same postMessage boundary as the production
// playground.bynk-lang.org / sandbox.bynk-lang.org split.
//
// Build for these origins first:
//   BYNK_APP_ORIGIN=http://localhost:8080 BYNK_SANDBOX_ORIGIN=http://localhost:8081 node build.mjs
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { extname, join, normalize } from "node:path";

const ROOT = new URL("./dist/", import.meta.url).pathname;
const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".wasm": "application/wasm",
  ".map": "application/json",
  ".scm": "text/plain; charset=utf-8",
};

function serve(port) {
  createServer(async (req, res) => {
    const url = new URL(req.url ?? "/", "http://localhost");
    let path = decodeURIComponent(url.pathname);
    if (path === "/" || path.endsWith("/")) path += "index.html";
    const file = join(ROOT, normalize(path).replace(/^(\.\.[/\\])+/, ""));
    try {
      const body = await readFile(file);
      res.writeHead(200, { "content-type": TYPES[extname(file)] ?? "application/octet-stream" });
      res.end(body);
    } catch {
      res.writeHead(404, { "content-type": "text/plain" });
      res.end("not found");
    }
  }).listen(port, () => console.log(`serving dist/ on http://localhost:${port}`));
}

serve(8080); // app origin
serve(8081); // sandbox origin
