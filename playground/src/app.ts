// The Bynk playground app (in-browser track, slice 4). Runs on the **app origin**:
// edits Bynk, compiles it to JS in wasm (`bynk_compile`), shows diagnostics, and
// dispatches a successful compile to the cross-origin **sandbox** iframe to run.

import { EditorState } from "@codemirror/state";
import { EditorView, keymap, lineNumbers, highlightActiveLine } from "@codemirror/view";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { bynkHighlighting } from "./highlight";
import { encodeSnippet, decodeSnippet } from "./deeplink";
import { SANDBOX_ORIGIN } from "./shared";
import type { CompileResult, Diagnostic, RunReply } from "./shared";
import init, { bynk_compile } from "./vendor/bynk_wasm.js";

const STARTER = `context playground.demo

consumes bynk { Clock, Logger }

service main {
  on call() -> Effect[Instant] given Clock, Logger {
    let _ <- Logger.info("Hello from Bynk — compiled to JS in your browser, no server.")
    let now <- Clock.now()
    now
  }
}
`;

const RUN_TIMEOUT_MS = 3000;
const PLATFORM_LOCK = new Set(["bynk.target.vendor_required", "bynk.target.browser_bundle_only"]);

const $ = (id: string) => document.getElementById(id)!;

let view: EditorView;
let sandboxReady = false;
let runSeq = 0;
const pending = new Map<number, (r: RunReply) => void>();

function source(): string {
  return view.state.doc.toString();
}

function setStatus(text: string, kind: "idle" | "busy" | "ok" | "error" = "idle"): void {
  const el = $("status");
  el.textContent = text;
  el.dataset.kind = kind;
}

function renderDiagnostics(diags: Diagnostic[]): void {
  const panel = $("diagnostics");
  panel.innerHTML = "";
  if (diags.length === 0) {
    panel.classList.add("empty");
    return;
  }
  panel.classList.remove("empty");
  for (const d of diags) {
    const row = document.createElement("div");
    row.className = `diag diag-${d.severity}`;
    const loc = d.line ? `${d.line}:${d.col}` : "—";
    // `category` is a fixed compiler constant today; escape it too so this stays
    // injection-proof if a future diagnostic ever derives a category from source.
    row.innerHTML = `<span class="diag-loc">${loc}</span> <span class="diag-cat">${escapeHtml(d.category)}</span> ${escapeHtml(d.message)}`;
    panel.appendChild(row);
  }
}

function escapeHtml(s: string): string {
  return s.replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" })[c] as string);
}

function renderOutput(reply: RunReply): void {
  const out = $("output");
  out.innerHTML = "";
  const add = (cls: string, text: string) => {
    const line = document.createElement("div");
    line.className = cls;
    line.textContent = text;
    out.appendChild(line);
  };
  if (reply.timedOut) {
    add("out-error", `⏱ execution exceeded ${RUN_TIMEOUT_MS} ms and was terminated.`);
    return;
  }
  if (reply.noEntry) {
    add("out-note", "Compiled. No zero-argument service handler to run — add `on call() -> …` to see output.");
    return;
  }
  for (const log of reply.logs) {
    add(log.level === "error" ? "out-error" : "out-log", log.message);
  }
  if (reply.error) {
    add("out-error", reply.error);
  } else if (reply.value !== undefined) {
    add("out-value", `⇒ ${reply.value}`);
  }
}

function showUnsupported(diags: Diagnostic[]): void {
  const lock = diags.find((d) => PLATFORM_LOCK.has(d.category));
  const banner = $("unsupported");
  if (lock) {
    banner.textContent = `Not runnable in-browser: ${lock.message}`;
    banner.hidden = false;
  } else {
    banner.hidden = true;
  }
}

async function compileAndRun(): Promise<void> {
  setStatus("compiling…", "busy");
  $("output").innerHTML = "";
  let result: CompileResult;
  try {
    result = JSON.parse(bynk_compile(source())) as CompileResult;
  } catch (e) {
    setStatus("compiler error", "error");
    renderDiagnostics([
      { path: null, line: 0, col: 0, severity: "error", category: "bynk.wasm", message: String(e) },
    ]);
    return;
  }
  renderDiagnostics(result.diagnostics);
  showUnsupported(result.diagnostics);

  if (!result.ok) {
    setStatus(`${result.diagnostics.filter((d) => d.severity === "error").length} error(s)`, "error");
    return;
  }
  if (!sandboxReady) {
    setStatus("sandbox not ready", "error");
    return;
  }

  setStatus("running…", "busy");
  const id = ++runSeq;
  const reply = await new Promise<RunReply>((resolve) => {
    pending.set(id, resolve);
    const iframe = $("sandbox") as HTMLIFrameElement;
    iframe.contentWindow!.postMessage(
      { kind: "bynk-run", id, files: result.files, timeoutMs: RUN_TIMEOUT_MS },
      // Opaque sandbox origin can't be named; the sandbox validates by app origin.
      "*",
    );
  });
  renderOutput(reply);
  setStatus(reply.error || reply.timedOut ? "run failed" : "ran", reply.error || reply.timedOut ? "error" : "ok");
}

async function share(): Promise<void> {
  const frag = await encodeSnippet(source());
  location.hash = frag;
  try {
    await navigator.clipboard.writeText(location.href);
    setStatus("link copied", "ok");
  } catch {
    setStatus("link in address bar", "ok");
  }
}

function makeEditor(doc: string): void {
  view = new EditorView({
    parent: $("editor"),
    state: EditorState.create({
      doc,
      extensions: [
        lineNumbers(),
        highlightActiveLine(),
        history(),
        keymap.of([
          { key: "Mod-Enter", run: () => (void compileAndRun(), true) },
          ...defaultKeymap,
          ...historyKeymap,
        ]),
        bynkHighlighting(),
        EditorView.theme({ "&": { height: "100%" }, ".cm-scroller": { overflow: "auto" } }, { dark: true }),
      ],
    }),
  });
}

function mountSandbox(): void {
  const iframe = document.createElement("iframe");
  iframe.id = "sandbox";
  // Distinct origin + opaque sandbox: even a sandbox escape lands on a bare origin.
  iframe.setAttribute("sandbox", "allow-scripts");
  iframe.src = `${SANDBOX_ORIGIN}/sandbox.html`;
  iframe.hidden = true;
  document.body.appendChild(iframe);

  window.addEventListener("message", (e: MessageEvent) => {
    const iframeWin = iframe.contentWindow;
    // Replies come from the opaque-origin iframe; identify it by window, not origin.
    if (e.source !== iframeWin) return;
    const data = e.data as { kind?: string; id?: number };
    if (data?.kind === "bynk-sandbox-ready") {
      sandboxReady = true;
      setStatus("ready", "ok");
      return;
    }
    if (data?.kind === "bynk-result" && typeof data.id === "number") {
      const resolve = pending.get(data.id);
      if (resolve) {
        pending.delete(data.id);
        resolve(data as RunReply);
      }
    }
  });
}

async function main(): Promise<void> {
  setStatus("loading…", "busy");
  mountSandbox();
  const fromHash = await decodeSnippet(location.hash);
  makeEditor(fromHash ?? STARTER);
  $("run").addEventListener("click", () => void compileAndRun());
  $("share").addEventListener("click", () => void share());
  // Load the wasm compiler. Resolve the module against the page URL (not
  // `import.meta.url`) so esbuild leaves the path alone and the `.wasm` is fetched
  // from the deploy root.
  await init(new URL("bynk_wasm_bg.wasm", location.href));
  if (!sandboxReady) setStatus("compiler ready", "ok");
}

void main();
