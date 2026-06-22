// Bynk debugging (v0.72, ADR 0104) — the one-click finale of the debugging track.
//
// Per ADR 0104 D1 this is *glue, not a Debug Adapter*: the extension contributes a
// `"bynk"` debug type whose DebugConfigurationProvider compiles + starts the V8
// inspector (by shelling the `--inspect` CLIs slices 2–3 shipped), reads the
// inspector port, and hands off to VS Code's built-in JavaScript debugger
// (`pwa-node`) via a delegated *attach*. The source maps slices 1/0.70 emit — whose
// `sources` are the `.bynk` files' absolute paths (so an editor breakpoint resolves
// to the same path the debugger loads) — do the breakpoint relocation.
//
// Two runtimes, one mechanism (both proven by the integration suite):
//   - test  → `bynkc test --inspect`  (Node `--inspect-brk`, type-stripped `.ts`)
//   - dev   → `bynk  dev  --inspect`  (workerd via `wrangler dev --inspector-port`)

import { spawn, ChildProcess } from "node:child_process";
import * as vscode from "vscode";

import { compilerPath } from "./tasks";
import { BYNK_DESCRIPTION_GENERATOR } from "./debugValues";

const BYNK_TYPE = "bynk";

/** Whether to render Bynk values in Bynk vocabulary in the debugger (slice 5).
 *  Default on; `bynk.debug.semanticValues: false` falls back to the raw shape. */
function semanticValuesEnabled(): boolean {
  return vscode.workspace
    .getConfiguration("bynk")
    .get<boolean>("debug.semanticValues", true);
}

/** The `bynk` front-end command — `bynk.bynkPath` setting, else `bynk` on PATH.
 *  Mirrors `compilerPath()` for `bynkc`; the `dev` debug path needs the driver. */
export function bynkPath(): string {
  return (
    vscode.workspace.getConfiguration("bynk").get<string>("bynkPath", "").trim() ||
    "bynk"
  );
}

// Files the debugger should step *over*, not into (ADR 0103 D5): Node internals,
// the emitted runtime/glue, and wrangler's bundling scratch. Keeps stepping inside
// the user's `.bynk`-derived code rather than the machinery beneath it.
const SKIP_FILES = [
  "<node_internals>/**",
  "**/runtime.ts",
  "**/runtime.js",
  "**/.wrangler/**",
  "**/node_modules/**",
];

export function registerDebug(context: vscode.ExtensionContext): void {
  // Each delegated `pwa-node` attach session is backed by a CLI child process
  // (bynkc/bynk → node/wrangler). Track it so terminating the debug session tears
  // the whole process group down — otherwise the inspected runtime is orphaned.
  const children = new Map<string, ChildProcess>();

  context.subscriptions.push(
    vscode.debug.registerDebugConfigurationProvider(
      BYNK_TYPE,
      new BynkDebugProvider(children),
    ),
    // The test path stops at the runtime entry (see `stopOnEntry` in the resolved
    // config) so js-debug binds pending `.bynk` breakpoints before any module
    // runs. That entry pause is the toolchain's, not the user's — auto-resume it,
    // only on our delegated sessions and only the `entry` reason, so the session
    // runs straight to the breakpoints the user set. Safe now: the entry stop
    // arrives *after* the attach is fully configured (no race), unlike a bare
    // resume on attach.
    vscode.debug.registerDebugAdapterTrackerFactory("*", {
      createDebugAdapterTracker(session: vscode.DebugSession) {
        if (!(session.configuration as { __bynkChild?: string })?.__bynkChild) {
          return undefined;
        }
        return {
          onDidSendMessage(m: { type?: string; event?: string; body?: { reason?: string; threadId?: number } }) {
            if (m.type === "event" && m.event === "stopped" && m.body?.reason === "entry") {
              void session.customRequest("continue", { threadId: m.body.threadId ?? 0 });
            }
          },
        };
      },
    }),
    vscode.debug.onDidTerminateDebugSession((s) => {
      const key = (s.configuration as { __bynkChild?: string })?.__bynkChild;
      if (key) killChild(children, key);
    }),
    { dispose: () => children.forEach((_, k) => killChild(children, k)) },
  );
}

/** Start a Bynk test-debug session at `root`: shells `bynkc test --inspect`,
 *  attaches, and a breakpoint in a test body or the code it exercises pauses.
 *  The Test Explorer's Debug profile (testing.ts) routes here via `startDebugging`. */
export async function debugBynkTests(): Promise<void> {
  await vscode.debug.startDebugging(undefined, {
    type: BYNK_TYPE,
    request: "launch",
    name: "Debug Bynk tests",
    mode: "test",
  });
}

class BynkDebugProvider implements vscode.DebugConfigurationProvider {
  constructor(private readonly children: Map<string, ChildProcess>) {}

  // A `"bynk"` config (from launch.json or `debugBynkTests`) is *resolved* into a
  // `pwa-node` attach: we launch the matching `--inspect` CLI, learn its inspector
  // port, and return a delegated Node attach. Returning a config of a different
  // type makes VS Code start *that* debugger — exactly the hand-off ADR 0104 wants.
  async resolveDebugConfiguration(
    folder: vscode.WorkspaceFolder | undefined,
    config: vscode.DebugConfiguration,
  ): Promise<vscode.DebugConfiguration | undefined> {
    // An empty `launch.json` (or a bare F5) gives us a typeless config — default
    // it to a test-debug at the workspace, the headline flow.
    const mode: "test" | "dev" = config.mode === "dev" ? "dev" : "test";
    const root = folder?.uri ?? (await findProjectRoot());
    if (!root) {
      void vscode.window.showErrorMessage(
        "Bynk debug: no bynk.toml found in the workspace.",
      );
      return undefined;
    }

    try {
      const { port, key } =
        mode === "dev"
          ? await this.startDev(root, config)
          : await this.startTest(root);
      return {
        type: "node",
        request: "attach",
        name: config.name || (mode === "dev" ? "Bynk dev" : "Bynk tests"),
        port,
        cwd: root.fsPath,
        // Stop at the runtime entry so js-debug is fully attached and tracking
        // pending breakpoints *before* any module loads — its pause-on-source-map
        // then binds a `.bynk` breakpoint as the test module parses, beating
        // execution. (Without this, `--inspect-brk` auto-resumes on attach and a
        // fast test runs to completion before the breakpoint binds.) The entry
        // pause is the toolchain's, not the user's — we auto-resume it below.
        stopOnEntry: mode === "test",
        // Resolve source maps wherever the emitted output lives (the default
        // confines resolution to the workspace folder; `.bynk/dev` and `out/` are
        // under it, but be explicit — the spike showed this is load-bearing).
        resolveSourceMapLocations: null,
        // Pre-scan the emitted output (which is `.ts`, not the `.js` js-debug
        // globs by default) so a `.bynk` breakpoint is bound *by URL* before its
        // module parses. Without this, binding waits for the script to load at
        // runtime — fine for the long-lived dev worker, but a fast test can run
        // to completion before the breakpoint binds.
        outFiles: [`${root.fsPath}/**/*.ts`, "!**/node_modules/**"],
        skipFiles: SKIP_FILES,
        // Render Bynk's tagged ADT values (`Ok(42)`, not `{tag:"Ok",…}`) — slice 5.
        // Node only: `workerd` rejects the in-debuggee evaluation this needs and
        // breaks *all* variable reading, so the dev path never sets it (the spike
        // proved both). Off when the user opts out via `bynk.debug.semanticValues`.
        ...(mode === "test" && semanticValuesEnabled()
          ? { customDescriptionGenerator: BYNK_DESCRIPTION_GENERATOR }
          : {}),
        // Tear down the CLI child when this attach session ends.
        __bynkChild: key,
      } as vscode.DebugConfiguration;
    } catch (e) {
      void vscode.window.showErrorMessage(`Bynk debug: ${String(e)}`);
      return undefined;
    }
  }

  /** `bynkc test --inspect` launches `node --inspect-brk`, which prints its
   *  `ws://host:port/…` inspector URL to stderr and pauses until we attach. We
   *  read the port from that line. */
  private async startTest(root: vscode.Uri): Promise<{ port: number; key: string }> {
    const child = spawnCli(compilerPath(), ["test", ".", "--inspect"], root.fsPath);
    const key = trackChild(this.children, child);
    try {
      const port = await waitForInspectorUrl(child, 30_000);
      return { port, key };
    } catch (e) {
      killChild(this.children, key);
      throw e;
    }
  }

  /** `bynk dev --inspect --inspector-port N` serves the worker under wrangler's V8
   *  inspector on a port we choose; we wait for its CDP discovery endpoint to come
   *  up, then attach. (`--context` selects the worker in a multi-context project.) */
  private async startDev(
    root: vscode.Uri,
    config: vscode.DebugConfiguration,
  ): Promise<{ port: number; key: string }> {
    const port = typeof config.port === "number" ? config.port : 9229;
    const args = ["dev", "--inspect", "--inspect-port", String(port)];
    if (typeof config.context === "string" && config.context) {
      args.push("--context", config.context);
    }
    const child = spawnCli(bynkPath(), args, root.fsPath);
    const key = trackChild(this.children, child);
    try {
      await waitForInspector(port, child, 60_000);
      return { port, key };
    } catch (e) {
      killChild(this.children, key);
      throw e;
    }
  }
}

// ---------------------------------------------------------------------------
// Process + inspector plumbing
// ---------------------------------------------------------------------------

/** Spawn a CLI in its own process group (`detached`) so killing it later takes
 *  the whole tree (bynkc→node, bynk→wrangler→workerd) with it. */
function spawnCli(command: string, args: string[], cwd: string): ChildProcess {
  return spawn(command, args, { cwd, detached: true, stdio: ["ignore", "pipe", "pipe"] });
}

let childSeq = 0;
function trackChild(children: Map<string, ChildProcess>, child: ChildProcess): string {
  const key = `bynk-dbg-${++childSeq}`;
  children.set(key, child);
  return key;
}

function killChild(children: Map<string, ChildProcess>, key: string): void {
  const child = children.get(key);
  children.delete(key);
  if (!child || child.killed || child.pid === undefined) return;
  try {
    // Negative pid → signal the whole process group (see `detached` above).
    process.kill(-child.pid, "SIGTERM");
  } catch {
    try {
      child.kill("SIGTERM");
    } catch {
      /* already gone */
    }
  }
}

/** Resolve with the inspector port parsed from a `ws://host:port/…` line on the
 *  child's stderr (what `node --inspect-brk` prints), or reject on timeout / early
 *  exit. */
function waitForInspectorUrl(child: ChildProcess, timeoutMs: number): Promise<number> {
  return new Promise((resolve, reject) => {
    let buf = "";
    const timer = setTimeout(() => {
      cleanup();
      reject(new Error("timed out waiting for the inspector URL"));
    }, timeoutMs);
    const onData = (b: Buffer) => {
      buf += String(b);
      const m = buf.match(/ws:\/\/[^:/]+:(\d+)\//);
      if (m) {
        cleanup();
        resolve(Number(m[1]));
      }
    };
    const onExit = () => {
      cleanup();
      reject(new Error(`the inspector process exited before it was ready\n${buf}`));
    };
    const cleanup = () => {
      clearTimeout(timer);
      child.stderr?.off("data", onData);
      child.off("exit", onExit);
    };
    child.stderr?.on("data", onData);
    child.once("exit", onExit);
  });
}

/** Resolve once wrangler's CDP discovery endpoint (`/json`) on `port` answers, or
 *  reject on timeout / early child exit. */
async function waitForInspector(
  port: number,
  child: ChildProcess,
  timeoutMs: number,
): Promise<void> {
  let exited = false;
  child.once("exit", () => {
    exited = true;
  });
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (exited) throw new Error("the dev server exited before the inspector was ready");
    try {
      const r = await fetch(`http://127.0.0.1:${port}/json`);
      const targets = (await r.json()) as unknown[];
      if (Array.isArray(targets) && targets.length > 0) return;
    } catch {
      /* not up yet */
    }
    await new Promise((r) => setTimeout(r, 400));
  }
  throw new Error(`timed out waiting for the inspector on port ${port}`);
}

/** The directory of the nearest `bynk.toml` — active `.bynk` file up, then the
 *  workspace-folder roots. Mirrors testing.ts / the LSP's `find_project_root`. */
async function findProjectRoot(): Promise<vscode.Uri | undefined> {
  const exists = async (uri: vscode.Uri): Promise<boolean> => {
    try {
      await vscode.workspace.fs.stat(uri);
      return true;
    } catch {
      return false;
    }
  };
  const active = vscode.window.activeTextEditor?.document;
  if (active?.languageId === "bynk" && active.uri.scheme === "file") {
    let dir = vscode.Uri.joinPath(active.uri, "..");
    for (;;) {
      if (await exists(vscode.Uri.joinPath(dir, "bynk.toml"))) return dir;
      const parent = vscode.Uri.joinPath(dir, "..");
      if (parent.path === dir.path) break;
      dir = parent;
    }
  }
  for (const folder of vscode.workspace.workspaceFolders ?? []) {
    if (await exists(vscode.Uri.joinPath(folder.uri, "bynk.toml"))) return folder.uri;
  }
  return undefined;
}
