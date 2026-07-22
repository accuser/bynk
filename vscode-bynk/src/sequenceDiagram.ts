// #846: "Bynk: Show Sequence Diagram" — the extension's first webview panel.
//
// Issues the `bynk/sequenceModel` custom LSP request (this server's first
// non-standard request — see `bynk-lsp/src/sequence_request.rs`) for the
// handler at the invoking CodeLens/command position, renders the result as
// a Mermaid sequence diagram, and handles click-to-code: the webview posts
// plain JSON `{uri, range}` (never real VS Code instances — a webview has
// no access to the `vscode` module), hydrated here into a real
// `vscode.Uri`/`Range` and revealed — the same hydration posture
// `codelens.ts` uses for `editor.action.showReferences`, just travelling
// the opposite direction (webview -> host, not server -> client).

import * as vscode from "vscode";
import type { LanguageClient } from "vscode-languageclient/node";

import type { RevealMessage, SequenceModel, SequencePayload } from "./webview/types";

interface SequenceModelParams {
  textDocument: { uri: string };
  position: { line: number; character: number };
}

let panel: vscode.WebviewPanel | undefined;

export function registerSequenceDiagram(
  context: vscode.ExtensionContext,
  getClient: () => LanguageClient | undefined,
): void {
  context.subscriptions.push(
    vscode.commands.registerCommand(
      "bynk.showSequenceDiagram",
      (uriArg?: string | vscode.Uri, positionArg?: { line: number; character: number }) =>
        void showSequenceDiagram(context, getClient, uriArg, positionArg),
    ),
  );
}

async function showSequenceDiagram(
  context: vscode.ExtensionContext,
  getClient: () => LanguageClient | undefined,
  uriArg?: string | vscode.Uri,
  positionArg?: { line: number; character: number },
): Promise<void> {
  const client = getClient();
  if (!client) {
    void vscode.window.showErrorMessage(
      "Bynk: Show Sequence Diagram needs the language server running.",
    );
    return;
  }

  let uri: vscode.Uri;
  let position: vscode.Position;
  if (uriArg !== undefined && positionArg !== undefined) {
    uri = typeof uriArg === "string" ? vscode.Uri.parse(uriArg) : uriArg;
    position = new vscode.Position(positionArg.line, positionArg.character);
  } else {
    const editor = vscode.window.activeTextEditor;
    if (!editor || editor.document.languageId !== "bynk") {
      void vscode.window.showErrorMessage(
        "Bynk: Show Sequence Diagram needs a .bynk file open (or a Show Sequence lens).",
      );
      return;
    }
    uri = editor.document.uri;
    position = editor.selection.active;
  }

  const params: SequenceModelParams = {
    textDocument: { uri: uri.toString() },
    position: { line: position.line, character: position.character },
  };

  let model: SequenceModel | null;
  try {
    model = await client.sendRequest<SequenceModel | null>("bynk/sequenceModel", params);
  } catch (err) {
    void vscode.window.showErrorMessage(`Bynk: Show Sequence Diagram failed: ${String(err)}`);
    return;
  }
  if (!model) {
    void vscode.window.showInformationMessage("Bynk: no handler found at the cursor.");
    return;
  }

  showPanel(context, { model, uri: uri.toString() });
}

function showPanel(context: vscode.ExtensionContext, payload: SequencePayload): void {
  if (!panel) {
    panel = vscode.window.createWebviewPanel(
      "bynkSequenceDiagram",
      "Bynk Sequence Diagram",
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
    panel.webview.onDidReceiveMessage((message: RevealMessage) => {
      if (message?.type === "reveal") void revealLocation(message.uri, message.range);
    });
  }
  panel.title = "Bynk Sequence Diagram";
  panel.webview.html = renderHtml(panel.webview, context, payload);
  panel.reveal(vscode.ViewColumn.Beside, true);
}

async function revealLocation(uriStr: string, range: RevealMessage["range"]): Promise<void> {
  const uri = vscode.Uri.parse(uriStr);
  const vsRange = new vscode.Range(
    new vscode.Position(range.start.line, range.start.character),
    new vscode.Position(range.end.line, range.end.character),
  );
  const doc = await vscode.workspace.openTextDocument(uri);
  const editor = await vscode.window.showTextDocument(doc, {
    viewColumn: vscode.ViewColumn.One,
    preserveFocus: false,
  });
  editor.selection = new vscode.Selection(vsRange.start, vsRange.start);
  editor.revealRange(vsRange, vscode.TextEditorRevealType.InCenterIfOutsideViewport);
}

function renderHtml(
  webview: vscode.Webview,
  context: vscode.ExtensionContext,
  payload: SequencePayload,
): string {
  const scriptUri = webview.asWebviewUri(
    vscode.Uri.joinPath(context.extensionUri, "out", "webview.js"),
  );
  const nonce = getNonce();
  // `<` -> `<` so a `</script>`-shaped label/name in the model can't
  // break out of the embedding `<script>` tag.
  const modelJson = JSON.stringify(payload).replace(/</g, "\\u003c");
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src ${webview.cspSource} data:; style-src ${webview.cspSource} 'unsafe-inline'; script-src 'nonce-${nonce}';">
<title>Bynk Sequence Diagram</title>
<style>
  body { font-family: var(--vscode-font-family, sans-serif); color: var(--vscode-foreground); padding: 8px; }
  #root svg { max-width: 100%; }
</style>
</head>
<body>
<div id="root">Loading…</div>
<script nonce="${nonce}">window.__BYNK_SEQUENCE_MODEL__ = ${modelJson};</script>
<script nonce="${nonce}" src="${scriptUri.toString()}"></script>
</body>
</html>`;
}

function getNonce(): string {
  const possible = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
  let text = "";
  for (let i = 0; i < 32; i++) {
    text += possible.charAt(Math.floor(Math.random() * possible.length));
  }
  return text;
}
