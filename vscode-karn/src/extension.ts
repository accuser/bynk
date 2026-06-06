// Karn VS Code extension entry point.
//
// Activates on .karn files or workspaces containing karn.toml. Spawns the
// karnc-lsp binary and connects to it as an LSP client. Adds two status-
// bar items: the project name (read from karn.toml) and the bundled
// karnc-lsp version. Syntax highlighting is provided by the bundled
// TextMate grammar (see syntaxes/karn.tmLanguage.json); the tree-sitter
// grammar is shipped alongside but registered separately via VS Code's
// tree-sitter integration when available.

import * as path from "node:path";
import * as fs from "node:fs";
import * as cp from "node:child_process";

import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;
let projectNameItem: vscode.StatusBarItem | undefined;
let compilerVersionItem: vscode.StatusBarItem | undefined;

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  const output = vscode.window.createOutputChannel("Karn LSP");

  const serverPath = resolveLspBinary();
  if (!serverPath) {
    void vscode.window.showErrorMessage(
      "Karn: cannot find karnc-lsp on PATH. Set the karn.executablePath setting or install the binary.",
    );
    return;
  }

  const serverOptions: ServerOptions = {
    run: { command: serverPath, transport: TransportKind.stdio },
    debug: { command: serverPath, transport: TransportKind.stdio },
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "karn" }],
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher("**/*.karn"),
      configurationSection: "karn",
    },
    outputChannel: output,
  };

  client = new LanguageClient("karn", "Karn LSP", serverOptions, clientOptions);

  try {
    await client.start();
  } catch (e) {
    output.appendLine(`Failed to start karnc-lsp: ${String(e)}`);
    void vscode.window.showErrorMessage(
      `Karn: failed to start LSP server: ${String(e)}`,
    );
    return;
  }

  // Status bar: project name (from karn.toml) + compiler version.
  projectNameItem = vscode.window.createStatusBarItem(
    vscode.StatusBarAlignment.Left,
    100,
  );
  projectNameItem.command = "karn.openProjectConfig";
  context.subscriptions.push(projectNameItem);

  compilerVersionItem = vscode.window.createStatusBarItem(
    vscode.StatusBarAlignment.Left,
    99,
  );
  context.subscriptions.push(compilerVersionItem);

  const updateStatus = () => updateStatusBar(serverPath);
  context.subscriptions.push(
    vscode.window.onDidChangeActiveTextEditor(updateStatus),
  );
  context.subscriptions.push(
    vscode.workspace.onDidChangeWorkspaceFolders(updateStatus),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand("karn.openProjectConfig", async () => {
      const config = await findKarnToml();
      if (config) {
        const doc = await vscode.workspace.openTextDocument(config);
        await vscode.window.showTextDocument(doc);
      } else {
        void vscode.window.showInformationMessage(
          "No karn.toml found in the current workspace.",
        );
      }
    }),
  );

  updateStatus();
}

export async function deactivate(): Promise<void> {
  if (client) {
    await client.stop();
  }
}

function resolveLspBinary(): string | undefined {
  const config = vscode.workspace.getConfiguration("karn");
  const configured = config.get<string>("executablePath", "karnc-lsp");
  // If the user gave an absolute or workspace-relative path, honor it.
  if (path.isAbsolute(configured) && fs.existsSync(configured)) {
    return configured;
  }
  // Otherwise search PATH.
  return findOnPath(configured);
}

function findOnPath(bin: string): string | undefined {
  const PATH = process.env.PATH ?? "";
  const sep = process.platform === "win32" ? ";" : ":";
  for (const dir of PATH.split(sep)) {
    const candidate = path.join(dir, bin);
    if (fs.existsSync(candidate)) {
      return candidate;
    }
    if (process.platform === "win32" && fs.existsSync(candidate + ".exe")) {
      return candidate + ".exe";
    }
  }
  // Fall back to just returning the name; spawn() will reject if unfound.
  return bin;
}

async function findKarnToml(): Promise<vscode.Uri | undefined> {
  const folders = vscode.workspace.workspaceFolders ?? [];
  for (const folder of folders) {
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
  if (!tomlUri) {
    return undefined;
  }
  try {
    const buf = await vscode.workspace.fs.readFile(tomlUri);
    const text = Buffer.from(buf).toString("utf8");
    const m = text.match(/^\s*name\s*=\s*"([^"]+)"/m);
    return m?.[1];
  } catch {
    return undefined;
  }
}

function readCompilerVersion(serverPath: string): string | undefined {
  try {
    const out = cp
      .spawnSync(serverPath, ["--version"], { timeout: 2000 })
      .stdout?.toString("utf8")
      .trim();
    return out;
  } catch {
    return undefined;
  }
}

async function updateStatusBar(serverPath: string): Promise<void> {
  const editor = vscode.window.activeTextEditor;
  const show = editor?.document.languageId === "karn";
  if (!projectNameItem || !compilerVersionItem) {
    return;
  }
  if (!show) {
    projectNameItem.hide();
    compilerVersionItem.hide();
    return;
  }
  const name = await readProjectName();
  projectNameItem.text = `$(symbol-package) ${name ?? "no project"}`;
  projectNameItem.tooltip = name
    ? "Open karn.toml"
    : "No karn.toml found in this workspace";
  projectNameItem.show();

  // `karnc-lsp --version` prints "karnc-lsp <version>"; show it verbatim so the
  // label honestly names the binary it queried (the LSP server).
  const version = readCompilerVersion(serverPath);
  compilerVersionItem.text = `$(symbol-misc) ${version ?? "karnc-lsp ?"}`;
  compilerVersionItem.show();
}
