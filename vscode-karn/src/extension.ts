// Karn VS Code extension entry point.
//
// Activates on .karn files or workspaces containing karn.toml. Provisions the
// karnc-lsp language server (see server.ts: setting → PATH → cached → download)
// and connects to it as an LSP client. If no server can be provisioned the
// failure is loud and actionable (error toast + a status-bar item + commands to
// retry), rather than silently degrading to grammar-only highlighting.

import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

import {
  downloadServer,
  readServerVersion,
  resolveExistingServer,
  serverVersion,
  targetTriple,
  type ResolvedServer,
} from "./server";
import { newContext, newProject } from "./scaffold";
import { registerTasks } from "./tasks";
import { provideCodeLenses } from "./codelens";

let client: LanguageClient | undefined;
let output: vscode.OutputChannel;
let projectNameItem: vscode.StatusBarItem | undefined;
let serverItem: vscode.StatusBarItem | undefined;

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  output = vscode.window.createOutputChannel("Karn LSP");

  projectNameItem = vscode.window.createStatusBarItem(
    vscode.StatusBarAlignment.Left,
    100,
  );
  projectNameItem.command = "karn.openProjectConfig";
  serverItem = vscode.window.createStatusBarItem(
    vscode.StatusBarAlignment.Left,
    99,
  );
  serverItem.command = "karn.showServerOutput";
  context.subscriptions.push(projectNameItem, serverItem);

  // Commands work whether or not the server is currently running, so register
  // them before the first start attempt — that way "Restart"/"Download" are
  // available to recover from a failed start.
  context.subscriptions.push(
    vscode.commands.registerCommand("karn.openProjectConfig", openProjectConfig),
    vscode.commands.registerCommand("karn.showServerOutput", () => output.show()),
    vscode.commands.registerCommand("karn.restartServer", () =>
      startServer(context, { interactive: true }),
    ),
    vscode.commands.registerCommand("karn.downloadServer", () =>
      startServer(context, { interactive: true, forceDownload: true }),
    ),
    // Scaffolding (B-2): work without the server, so register them eagerly too.
    vscode.commands.registerCommand("karn.newContext", () => newContext()),
    vscode.commands.registerCommand("karn.newProject", () => newProject()),
  );

  // B-2: the `karnc: check` build task (errors → Problems via `$karnc`).
  registerTasks(context);

  context.subscriptions.push(
    vscode.window.onDidChangeActiveTextEditor(() => updateProjectItem()),
    vscode.workspace.onDidChangeWorkspaceFolders(() => updateProjectItem()),
  );

  await startServer(context, { interactive: false });
}

/** Provision a server, (re)start the client, and reflect the result in the UI.
 *  Safe to call repeatedly (restart command). */
async function startServer(
  context: vscode.ExtensionContext,
  opts: { interactive: boolean; forceDownload?: boolean },
): Promise<void> {
  await stopClient();

  const resolved = await ensureServer(context, opts);
  if (!resolved) {
    setServerItem("error", "Karn LSP: not running");
    return;
  }

  const serverOptions: ServerOptions = {
    run: { command: resolved.path, transport: TransportKind.stdio },
    debug: { command: resolved.path, transport: TransportKind.stdio },
  };
  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "karn" }],
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher("**/*.karn"),
      configurationSection: "karn",
    },
    outputChannel: output,
    middleware: {
      // Client-side gate for the server's always-on inlay hints. The master
      // `karn.inlayHints.enable` suppresses them entirely (returning []); when
      // on, the per-kind `karn.inlayHints.types` / `.parameterNames` toggles
      // filter the result by the server-tagged `kind`. Takes effect on the next
      // request (edit/scroll). The built-in `editor.inlayHints.enabled` is the
      // instant, editor-wide toggle — these are the persistent Karn preferences.
      provideInlayHints: async (document, viewPort, token, next) => {
        const cfg = vscode.workspace.getConfiguration("karn");
        if (!cfg.get<boolean>("inlayHints.enable", true)) return [];
        const showTypes = cfg.get<boolean>("inlayHints.types", true);
        const showParams = cfg.get<boolean>("inlayHints.parameterNames", true);
        const hints = await next(document, viewPort, token);
        if (!hints || (showTypes && showParams)) return hints;
        return hints.filter((h) =>
          h.kind === vscode.InlayHintKind.Type
            ? showTypes
            : h.kind === vscode.InlayHintKind.Parameter
              ? showParams
              : true,
        );
      },
      // The reference-count CodeLens carries `editor.action.showReferences`,
      // whose arguments must be real VS Code instances; the server sends them
      // as plain LSP JSON, so re-hydrate them client-side.
      provideCodeLenses,
    },
  };

  client = new LanguageClient("karn", "Karn LSP", serverOptions, clientOptions);
  try {
    await client.start();
  } catch (e) {
    output.appendLine(`[server] failed to start: ${String(e)}`);
    void vscode.window
      .showErrorMessage(
        `Karn: the language server failed to start (${resolved.source}). See the Karn LSP output.`,
        "Show Output",
        "Restart",
      )
      .then((pick) => {
        if (pick === "Show Output") output.show();
        if (pick === "Restart") void startServer(context, { interactive: true });
      });
    setServerItem("error", "Karn LSP: failed");
    return;
  }

  checkVersionMatch(context, resolved.path);
  setServerItem("ok", `Karn LSP (${resolved.source})`);
  updateProjectItem();
}

/** Resolve an existing server or, when nothing is configured and the platform
 *  is supported, download one. Returns undefined (after surfacing a clear,
 *  actionable error) when no server can be provisioned. */
async function ensureServer(
  context: vscode.ExtensionContext,
  opts: { interactive: boolean; forceDownload?: boolean },
): Promise<ResolvedServer | undefined> {
  if (!opts.forceDownload) {
    const existing = resolveExistingServer(context);
    if (existing) return existing;
  }

  const configured = vscode.workspace
    .getConfiguration("karn")
    .get<string>("executablePath", "")
    .trim();
  if (configured && !opts.forceDownload) {
    // An explicit setting that doesn't resolve: don't paper over it with a
    // download — tell the user their setting is wrong.
    await reportNoServer(
      `Karn: \`karn.executablePath\` is set to "${configured}", but no such executable was found.`,
      context,
    );
    return undefined;
  }

  if (!targetTriple()) {
    await reportNoServer(
      `Karn: no prebuilt language server for ${process.platform}/${process.arch}. ` +
        "Build it with `cargo build --release -p karn-lsp` and set `karn.executablePath`.",
      context,
    );
    return undefined;
  }

  try {
    const path = await downloadServer(context, output);
    return { path, source: "downloaded" };
  } catch (e) {
    output.appendLine(`[server] download failed: ${String(e)}`);
    await reportNoServer(
      `Karn: couldn't download the language server (${serverVersion(context)}). ` +
        "It may not be released yet, or the network is unavailable.",
      context,
    );
    return undefined;
  }
}

async function reportNoServer(
  message: string,
  context: vscode.ExtensionContext,
): Promise<void> {
  const pick = await vscode.window.showErrorMessage(
    message,
    "Download Server",
    "Open Settings",
    "Show Output",
  );
  if (pick === "Download Server") {
    await startServer(context, { interactive: true, forceDownload: true });
  } else if (pick === "Open Settings") {
    await vscode.commands.executeCommand(
      "workbench.action.openSettings",
      "karn.executablePath",
    );
  } else if (pick === "Show Output") {
    output.show();
  }
}

/** Warn (non-blocking) if the running server's version disagrees with the one
 *  this extension build expects. */
function checkVersionMatch(
  context: vscode.ExtensionContext,
  serverPath: string,
): void {
  const reported = readServerVersion(serverPath); // "karnc-lsp 0.23.0"
  const expected = serverVersion(context).replace(/^v/, ""); // "0.23.0"
  if (reported && !reported.includes(expected)) {
    output.appendLine(
      `[server] version note: running "${reported}", extension expects ${expected}`,
    );
    void vscode.window.showWarningMessage(
      `Karn: language server is "${reported}" but this extension expects ${expected}. ` +
        "Consider running “Karn: Download Language Server”.",
    );
  }
}

async function stopClient(): Promise<void> {
  if (client) {
    try {
      await client.stop();
    } catch {
      /* already down */
    }
    client = undefined;
  }
}

export async function deactivate(): Promise<void> {
  await stopClient();
}

// ---------------------------------------------------------------------------
// Status bar + project config
// ---------------------------------------------------------------------------

function setServerItem(state: "ok" | "error", text: string): void {
  if (!serverItem) return;
  const icon = state === "ok" ? "$(check)" : "$(error)";
  serverItem.text = `${icon} ${text}`;
  serverItem.tooltip =
    state === "ok"
      ? "Karn language server is running — click to show its output"
      : "Karn language server is not running — click to show its output";
  serverItem.backgroundColor =
    state === "error"
      ? new vscode.ThemeColor("statusBarItem.warningBackground")
      : undefined;
  updateProjectItem();
}

function updateProjectItem(): void {
  const show =
    vscode.window.activeTextEditor?.document.languageId === "karn";
  if (!projectNameItem || !serverItem) return;
  if (!show) {
    projectNameItem.hide();
    serverItem.hide();
    return;
  }
  void readProjectName().then((name) => {
    projectNameItem!.text = `$(symbol-package) ${name ?? "no project"}`;
    projectNameItem!.tooltip = name
      ? "Open karn.toml"
      : "No karn.toml found in this workspace";
    projectNameItem!.show();
  });
  serverItem.show();
}

async function fileExists(uri: vscode.Uri): Promise<boolean> {
  try {
    await vscode.workspace.fs.stat(uri);
    return true;
  } catch {
    return false;
  }
}

async function findKarnToml(): Promise<vscode.Uri | undefined> {
  // Walk upward from the active `.karn` file to the nearest `karn.toml`,
  // mirroring the LSP's `find_project_root` (karn-lsp/src/main.rs) — so a
  // nested project (a `karn.toml` below the workspace-folder root) is found,
  // not just one at the root.
  const active = vscode.window.activeTextEditor?.document;
  if (active?.languageId === "karn" && active.uri.scheme === "file") {
    let dir = vscode.Uri.joinPath(active.uri, "..");
    for (;;) {
      const candidate = vscode.Uri.joinPath(dir, "karn.toml");
      if (await fileExists(candidate)) return candidate;
      const parent = vscode.Uri.joinPath(dir, "..");
      if (parent.path === dir.path) break; // reached the filesystem root
      dir = parent;
    }
  }
  // Fall back to the workspace-folder roots (covers a root-level project and
  // the no-active-`.karn`-file case).
  for (const folder of vscode.workspace.workspaceFolders ?? []) {
    const candidate = vscode.Uri.joinPath(folder.uri, "karn.toml");
    if (await fileExists(candidate)) return candidate;
  }
  return undefined;
}

async function readProjectName(): Promise<string | undefined> {
  const tomlUri = await findKarnToml();
  if (!tomlUri) return undefined;
  try {
    const buf = await vscode.workspace.fs.readFile(tomlUri);
    const text = Buffer.from(buf).toString("utf8");
    return text.match(/^\s*name\s*=\s*"([^"]+)"/m)?.[1];
  } catch {
    return undefined;
  }
}

async function openProjectConfig(): Promise<void> {
  const config = await findKarnToml();
  if (config) {
    const doc = await vscode.workspace.openTextDocument(config);
    await vscode.window.showTextDocument(doc);
  } else {
    void vscode.window.showInformationMessage(
      "No karn.toml found in the current workspace.",
    );
  }
}
