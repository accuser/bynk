// Slice 3 (ADR 0104) CDP harness — driven by tests/dev_inspect.rs.
//
// Attaches to a running `wrangler dev` worker's inspector exactly as a JavaScript
// debugger would, sets a breakpoint mapped from a `.bynk` handler line through the
// bundle's composed source map, sends a request, and confirms it binds and pauses.
// Prints "BIND OK" and exits 0 on success; exits 1 otherwise.
//
// Usage: node wrangler_attach.mjs <inspector_port> <app_port> <bynk_line>
//
// Note: wrangler's inspector requires an `Origin` header on the WebSocket
// (`400 Bad Request` without it) — the one workerd-specific wrinkle of the attach.

import { readFileSync } from "node:fs";

const [, , inspectorPort, appPort, bynkLineArg] = process.argv;
const bynkLine = Number(bynkLineArg);
const B = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const decSeg = (s) => {
  const o = []; let sh = 0, v = 0;
  for (const c of s) { const d = B.indexOf(c); v += (d & 31) << sh; if (d & 32) sh += 5; else { o.push(v & 1 ? -(v >> 1) : v >> 1); v = 0; sh = 0; } }
  return o;
};
const fail = (m) => { console.error(m); process.exit(1); };

// Discover the CDP target.
const target = (await (await fetch(`http://127.0.0.1:${inspectorPort}/json`)).json())[0];
const ws = new WebSocket(target.webSocketDebuggerUrl, { headers: { Origin: "http://localhost" } });
const scripts = []; const pending = new Map(); const pausedQ = []; let pausedW = null; let id = 1;
const onPaused = (p) => { if (pausedW) { const w = pausedW; pausedW = null; w(p); } else pausedQ.push(p); };
const nextPaused = () => new Promise((r) => { if (pausedQ.length) r(pausedQ.shift()); else pausedW = r; });
ws.addEventListener("message", (e) => {
  const m = JSON.parse(e.data);
  if (m.id && pending.has(m.id)) { pending.get(m.id)(m.result); pending.delete(m.id); }
  else if (m.method === "Debugger.scriptParsed") scripts.push(m.params);
  else if (m.method === "Debugger.paused") onPaused(m.params);
});
const send = (method, params = {}) => { const i = id++; ws.send(JSON.stringify({ id: i, method, params })); return new Promise((r) => pending.set(i, r)); };
await Promise.race([
  new Promise((r) => ws.addEventListener("open", r)),
  new Promise((_, rej) => setTimeout(() => rej(new Error("ws never opened (Origin header?)")), 8000)),
]).catch((e) => fail(e.message));

await send("Runtime.enable");
await send("Debugger.enable");
await send("Runtime.runIfWaitingForDebugger").catch(() => {});
// Warm the worker so its bundle parses.
try { await fetch(`http://127.0.0.1:${appPort}/`); } catch {}
await new Promise((r) => setTimeout(r, 1500));

const worker = scripts.find((s) => /index\.js$/.test(s.url || "") && (s.sourceMapURL || "").length > 0);
if (!worker) fail("no worker bundle script with a source map");
const mapPath = decodeURIComponent(worker.url.replace("file://", "")).replace(/index\.js$/, worker.sourceMapURL);
const map = JSON.parse(readFileSync(mapPath, "utf8"));
const bynkIdx = map.sources.findIndex((s) => /\.bynk$/.test(s));
if (bynkIdx < 0) fail(`bundle map does not resolve to a .bynk source: ${JSON.stringify(map.sources)}`);

// Full-decode the bundle map; collect generated lines that map to the target
// `.bynk` line (esbuild composed handlers.ts.map -> .bynk, per-statement v0.70).
let sIdx = 0, sLine = 0, sCol = 0;
const targetGenLines = [];
map.mappings.split(";").forEach((line, g) => {
  if (!line) return;
  for (const seg of line.split(",")) {
    const f = decSeg(seg);
    if (f.length >= 4) { sIdx += f[1]; sLine += f[2]; sCol += f[3]; if (sIdx === bynkIdx && sLine + 1 === bynkLine) targetGenLines.push(g); }
  }
});
if (!targetGenLines.length) fail(`no bundle line maps to ${map.sources[bynkIdx]}:${bynkLine}`);
console.log(`[setup] .bynk:${bynkLine} -> ${targetGenLines.length} bundle line(s)`);

let bound = 0;
for (const g of targetGenLines) {
  const bp = await send("Debugger.setBreakpointByUrl", { lineNumber: g, url: worker.url });
  bound += bp && bp.locations ? bp.locations.length : 0;
}
console.log(`[bp] set on ${targetGenLines.length} line(s), ${bound} bound`);
if (!bound) fail("no breakpoint bound to a real location");

// Fire a request (do not await — the handler pauses at the breakpoint).
fetch(`http://127.0.0.1:${appPort}/`).catch(() => {});
const hit = await Promise.race([
  nextPaused(),
  new Promise((_, rej) => setTimeout(() => rej(new Error("never paused at the breakpoint")), 8000)),
]).catch((e) => fail(e.message));

const stoppedGen = hit.callFrames[0].location.lineNumber;
const ok = targetGenLines.includes(stoppedGen) && hit.reason !== "exception";
await send("Debugger.resume").catch(() => {});
if (ok) { console.log(`BIND OK: breakpoint mapped from .bynk:${bynkLine} bound and paused on the worker (bundle line ${stoppedGen + 1})`); process.exit(0); }
fail(`paused at bundle line ${stoppedGen + 1}, not a .bynk:${bynkLine} line`);
