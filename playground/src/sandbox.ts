// The execution document — served from the **sandbox origin** and embedded by the
// app as `<iframe sandbox="allow-scripts">` (in-browser track, slice 4; ADR 0140).
//
// It is the safety boundary: it accepts a compiled JS module graph from the app
// origin (and only that origin), links it into blob-URL ES modules, and runs it in
// a **Web Worker** under a hard wall-clock budget — so an infinite loop or runaway
// is `terminate()`d, never wedging anything. `Fetch`/`Secrets` already throw in the
// browser binding (slice 2), so the sandbox cannot reach the network or secrets.
// The Worker captures `Logger` output and the entry's return value and posts them
// back; nothing else crosses the boundary.

import { APP_ORIGIN } from "./shared";
import type { EmittedFile, RunReply, RunRequest } from "./shared";

/** Resolve a relative import specifier against the importing module's path. */
function resolvePath(fromPath: string, spec: string): string {
  const baseDir = fromPath.includes("/") ? fromPath.slice(0, fromPath.lastIndexOf("/")) : "";
  const out: string[] = [];
  for (const seg of `${baseDir}/${spec}`.split("/")) {
    if (seg === "" || seg === ".") continue;
    if (seg === "..") out.pop();
    else out.push(seg);
  }
  return out.join("/");
}

const SPEC_RE = /(\bfrom\s*|\bimport\s*)(["'])([^"']+)\2/g;

/** Relative import specifiers a module declares (the edges of the graph). */
function importsOf(contents: string): string[] {
  const specs: string[] = [];
  for (const m of contents.matchAll(SPEC_RE)) {
    if (m[3].startsWith(".")) specs.push(m[3]);
  }
  return specs;
}

/**
 * Link the emitted module graph into blob-URL ES modules and return the entry
 * (`compose.js`) blob URL. The graph is a DAG (runtime → bynk → binding → context
 * → compose), so we create blobs in dependency order, rewriting each module's
 * relative specifiers to the already-created blob URLs of its targets. (Workers
 * don't support import maps, so specifier rewriting is the portable approach.)
 */
function linkGraph(files: EmittedFile[]): { entry: string | null; urls: string[] } {
  const byPath = new Map<string, EmittedFile>();
  for (const f of files) byPath.set(f.path, f);

  const blobUrls = new Map<string, string>();
  const created: string[] = [];
  const visiting = new Set<string>();

  const build = (path: string): string => {
    const existing = blobUrls.get(path);
    if (existing) return existing;
    if (visiting.has(path)) throw new Error(`import cycle through ${path}`);
    const file = byPath.get(path);
    if (!file) throw new Error(`missing module ${path}`);
    visiting.add(path);

    let contents = file.contents;
    for (const spec of importsOf(contents)) {
      const target = resolvePath(path, spec);
      const url = build(target);
      // Replace the exact quoted specifier with the dependency's blob URL.
      contents = contents.split(`"${spec}"`).join(`"${url}"`).split(`'${spec}'`).join(`'${url}'`);
    }
    visiting.delete(path);
    const url = URL.createObjectURL(new Blob([contents], { type: "text/javascript" }));
    blobUrls.set(path, url);
    created.push(url);
    return url;
  };

  for (const f of files) build(f.path);
  const entry = byPath.has("compose.js") ? blobUrls.get("compose.js")! : null;
  return { entry: entry ?? null, urls: created };
}

// The Worker bootstrap (a module, instantiated from a blob): imports the linked
// entry, instantiates the composition root, invokes the single zero-argument
// service handler, and reports captured logs + the returned value. Console output
// (the `Logger` provider) is intercepted in-Worker.
const WORKER_SRC = `
const logs = [];
const fmt = (v) => { try { return typeof v === "string" ? v : JSON.stringify(v); } catch { return String(v); } };
console.log = (...a) => logs.push({ level: "info", message: a.map(fmt).join(" ") });
console.info = console.log;
console.error = (...a) => logs.push({ level: "error", message: a.map(fmt).join(" ") });
console.warn = console.error;
self.onmessage = async (e) => {
  const { entryUrl } = e.data;
  try {
    const mod = await import(entryUrl);
    if (typeof mod.composeApp !== "function") {
      self.postMessage({ logs, noEntry: true });
      return;
    }
    const app = mod.composeApp();
    let invoked = false, value;
    for (const ctxKey of Object.keys(app)) {
      const surface = app[ctxKey];
      if (!surface || typeof surface !== "object") continue;
      for (const m of Object.keys(surface)) {
        const fn = surface[m];
        if (typeof fn === "function" && fn.length === 0) {
          invoked = true;
          value = await fn();
          break;
        }
      }
      if (invoked) break;
    }
    if (!invoked) { self.postMessage({ logs, noEntry: true }); return; }
    self.postMessage({ logs, value: value === undefined ? undefined : fmt(value) });
  } catch (err) {
    self.postMessage({ logs, error: String((err && err.stack) || err) });
  }
};
`;

function reply(msg: RunReply): void {
  // The app is a real origin; target it precisely. (This document's own origin is
  // opaque under `sandbox=allow-scripts`, so the app validates by window identity.)
  parent.postMessage(msg, APP_ORIGIN);
}

function run(req: RunRequest): void {
  let entry: string | null;
  let urls: string[] = [];
  try {
    const linked = linkGraph(req.files);
    entry = linked.entry;
    urls = linked.urls;
  } catch (err) {
    reply({ kind: "bynk-result", id: req.id, logs: [], error: `link error: ${String(err)}` });
    return;
  }
  if (!entry) {
    reply({ kind: "bynk-result", id: req.id, logs: [], noEntry: true });
    return;
  }

  const workerUrl = URL.createObjectURL(new Blob([WORKER_SRC], { type: "text/javascript" }));
  const worker = new Worker(workerUrl, { type: "module" });
  let settled = false;
  const cleanup = () => {
    urls.forEach(URL.revokeObjectURL);
    URL.revokeObjectURL(workerUrl);
  };

  const timer = setTimeout(() => {
    if (settled) return;
    settled = true;
    worker.terminate();
    cleanup();
    reply({ kind: "bynk-result", id: req.id, logs: [], timedOut: true });
  }, req.timeoutMs);

  worker.onmessage = (e: MessageEvent) => {
    if (settled) return;
    settled = true;
    clearTimeout(timer);
    worker.terminate();
    cleanup();
    const d = e.data as { logs?: RunReply["logs"]; value?: string; error?: string; noEntry?: boolean };
    reply({
      kind: "bynk-result",
      id: req.id,
      logs: d.logs ?? [],
      value: d.value,
      error: d.error,
      noEntry: d.noEntry,
    });
  };
  worker.onerror = (e: ErrorEvent) => {
    if (settled) return;
    settled = true;
    clearTimeout(timer);
    worker.terminate();
    cleanup();
    reply({ kind: "bynk-result", id: req.id, logs: [], error: e.message || "worker error" });
  };

  worker.postMessage({ entryUrl: entry });
}

window.addEventListener("message", (e: MessageEvent) => {
  // Only ever act on the app origin's instructions.
  if (e.origin !== APP_ORIGIN) return;
  const data = e.data as RunRequest;
  if (data && data.kind === "bynk-run") run(data);
});

// Announce readiness so the app doesn't race the iframe load.
parent.postMessage({ kind: "bynk-sandbox-ready" }, APP_ORIGIN);
