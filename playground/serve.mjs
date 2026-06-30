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
// Same-origin share API (slice 5c): proxy `/api/*` on the app origin to the local
// `wrangler dev` Worker, mirroring the production Cloudflare route
// (`playground.bynk-lang.org/api/* → the snippets Worker`) so the browser never
// makes a cross-origin call (Bynk's HTTP surface emits no CORS).
const SHARE_WORKER = process.env.BYNK_SHARE_WORKER ?? "http://localhost:8799";
const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".wasm": "application/wasm",
  ".map": "application/json",
  ".scm": "text/plain; charset=utf-8",
};

async function proxyToWorker(req, res, url) {
  try {
    const body = ["GET", "HEAD"].includes(req.method ?? "GET")
      ? undefined
      : await new Promise((resolve) => {
          const chunks = [];
          req.on("data", (c) => chunks.push(c));
          req.on("end", () => resolve(Buffer.concat(chunks)));
        });
    const upstream = await fetch(SHARE_WORKER + url.pathname + url.search, {
      method: req.method,
      headers: { "content-type": req.headers["content-type"] ?? "application/json" },
      body,
    });
    const buf = Buffer.from(await upstream.arrayBuffer());
    res.writeHead(upstream.status, {
      "content-type": upstream.headers.get("content-type") ?? "application/json",
    });
    res.end(buf);
  } catch {
    res.writeHead(502, { "content-type": "text/plain" });
    res.end("share worker unavailable");
  }
}

function serve(port) {
  createServer(async (req, res) => {
    const url = new URL(req.url ?? "/", "http://localhost");
    if (url.pathname.startsWith("/api/")) return proxyToWorker(req, res, url);
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
