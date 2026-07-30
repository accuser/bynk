// #851: "Bynk: Show Architecture Map" — the whole-project architecture
// webview, the third consumer of the shared webview substrate (`webviewHost.ts`)
// #846 introduced and #847 extracted. Issues the `bynk/architectureModel`
// custom LSP request (project-scoped — see `bynk-lsp/src/architecture_request.rs`)
// for the active file's project and renders the result as a Mermaid `flowchart`.
//
// DECISION B (per-project, nearest `bynk.toml`): like the sequence/documentation
// views, this needs an active `.bynk` editor to resolve *which* project's round
// to read — the request's `textDocument` is that resolution key, not a scope
// restriction on the result (the model covers every context/adapter in that
// project, not just the active file).

import * as vscode from "vscode";
import type { LanguageClient } from "vscode-languageclient/node";

import { getNonce, renderWebviewHtml, revealLocation } from "./webviewHost";
import type { ArchModel, ArchPayload, ArchRevealMessage } from "./webview/architectureTypes";

interface ArchitectureModelParams {
  textDocument: { uri: string };
}

let panel: vscode.WebviewPanel | undefined;

export function registerArchitectureMap(
  context: vscode.ExtensionContext,
  getClient: () => LanguageClient | undefined,
): void {
  context.subscriptions.push(
    vscode.commands.registerCommand(
      "bynk.showArchitectureMap",
      () => void showArchitectureMap(context, getClient),
    ),
  );
}

async function showArchitectureMap(
  context: vscode.ExtensionContext,
  getClient: () => LanguageClient | undefined,
): Promise<void> {
  const client = getClient();
  if (!client) {
    void vscode.window.showErrorMessage(
      "Bynk: Show Architecture Map needs the language server running.",
    );
    return;
  }

  const editor = vscode.window.activeTextEditor;
  if (!editor || editor.document.languageId !== "bynk") {
    void vscode.window.showErrorMessage(
      "Bynk: Show Architecture Map needs a .bynk file open (to resolve the project).",
    );
    return;
  }
  const uri = editor.document.uri;

  const params: ArchitectureModelParams = { textDocument: { uri: uri.toString() } };
  let model: ArchModel | null;
  try {
    model = await client.sendRequest<ArchModel | null>("bynk/architectureModel", params);
  } catch (err) {
    void vscode.window.showErrorMessage(`Bynk: Show Architecture Map failed: ${String(err)}`);
    return;
  }
  if (!model) {
    void vscode.window.showInformationMessage(
      "Bynk: no architecture model for this project (not a project file, or no committed analysis yet).",
    );
    return;
  }

  showPanel(context, { model });
}

function showPanel(context: vscode.ExtensionContext, payload: ArchPayload): void {
  if (!panel) {
    panel = vscode.window.createWebviewPanel(
      "bynkArchitectureMap",
      "Bynk Architecture Map",
      vscode.ViewColumn.Beside,
      {
        enableScripts: true,
        retainContextWhenHidden: true,
        localResourceRoots: [vscode.Uri.joinPath(context.extensionUri, "out")],
      },
    );
    panel.onDidDispose(() => {
      panel = undefined;
    });
    panel.webview.onDidReceiveMessage((message: ArchRevealMessage) => {
      if (message?.type === "reveal") void revealLocation(message.uri, message.range);
    });
  }
  panel.title = "Bynk Architecture Map";
  panel.webview.html = renderHtml(panel.webview, context, payload);
  panel.reveal(vscode.ViewColumn.Beside, true);
}

function renderHtml(
  webview: vscode.Webview,
  context: vscode.ExtensionContext,
  payload: ArchPayload,
): string {
  const scriptUri = webview.asWebviewUri(
    vscode.Uri.joinPath(context.extensionUri, "out", "architecture.js"),
  );
  return renderWebviewHtml({
    webview,
    scriptUri,
    nonce: getNonce(),
    title: "Bynk Architecture Map",
    globalName: "__BYNK_ARCH_MODEL__",
    payload,
  });
}
