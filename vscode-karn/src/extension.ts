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
  );

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
      // Client-side gate for the server's always-on inlay hints. Returning
      // [] suppresses them when `karn.inlayHints.enable` is off; it takes
      // effect on the next request (edit/scroll). The built-in
      // `editor.inlayHints.enabled` is the instant, editor-wide toggle —
      // this one is the persistent per-language Karn preference.
      provideInlayHints: (document, viewPort, token, next) => {
        const enabled = vscode.workspace
          .getConfiguration("karn")
          .get<boolean>("inlayHints.enable", true);
        if (!enabled) return [];
        return next(document, viewPort, token);
      },
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

async function findKarnToml(): Promise<vscode.Uri | undefined> {
  for (const folder of vscode.workspace.workspaceFolders ?? []) {
    const candidate = vscode.Uri.joinPath(folder.uri, "karn.toml");
    try {
      await vscode.workspace.fs.stat(candidate);
      return candidate;
    } catch {
      /* continue */
    }
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
