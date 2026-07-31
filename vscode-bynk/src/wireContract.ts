// #855: "Bynk: Show Wire Contract" — the wire-contract-peek webview panel,
// the fourth consumer of the shared webview substrate (`webviewHost.ts`).
// Issues the `bynk/wireContract` custom LSP request (file-scoped + position,
// like #846's `bynk/sequenceModel` — see `bynk-lsp/src/wire_contract_request.rs`)
// for the handler at the cursor and renders the result as a JSON shape tree
// (no Mermaid, no markdown-it — see `webview/wire-render.ts`'s module doc).

import * as vscode from "vscode";
import type { LanguageClient } from "vscode-languageclient/node";

import { getNonce, renderWebviewHtml, revealLocation } from "./webviewHost";
import type { WcModel, WcPayload, WcRevealMessage } from "./webview/wireContractTypes";

interface WireContractParams {
  textDocument: { uri: string };
  position: { line: number; character: number };
}

// The panel's own styling, layered over the shared base in `renderWebviewHtml`
// (host-side, same posture as `documentationView.ts`'s `DOC_CSS` — the
// webview bundle itself injects no `<style>`, it only sets classes).
const WIRE_CSS = `
  h1 { display: flex; align-items: center; gap: 8px; cursor: pointer; font-family: var(--vscode-editor-font-family, monospace); }
  .wc-kind { color: var(--vscode-descriptionForeground); font-weight: normal; font-size: 0.75em; text-transform: uppercase; letter-spacing: 0.05em; }
  section.wc-card { border: 1px solid var(--vscode-panel-border); border-radius: 4px; padding: 8px 12px; margin: 12px 0; }
  section.wc-card > h2 { margin-top: 0; font-size: 0.95em; text-transform: uppercase; letter-spacing: 0.03em; color: var(--vscode-descriptionForeground); }
  .wc-muted { color: var(--vscode-descriptionForeground); font-style: italic; }
  .wc-hash { font-family: var(--vscode-editor-font-family, monospace); background: var(--vscode-textCodeBlock-background); padding: 2px 6px; border-radius: 3px; }
  code, pre { font-family: var(--vscode-editor-font-family, monospace); }
  pre { background: var(--vscode-textCodeBlock-background); padding: 8px; overflow-x: auto; border-radius: 3px; }
  table.wc-responses { border-collapse: collapse; width: 100%; }
  table.wc-responses th, table.wc-responses td { text-align: left; padding: 3px 8px; border-bottom: 1px solid var(--vscode-panel-border); }
  table.wc-responses tr.wc-row-muted { color: var(--vscode-descriptionForeground); font-style: italic; }
  table.wc-responses tr.wc-row-clickable { cursor: pointer; }
  a.type-chip { font-family: var(--vscode-editor-font-family, monospace); color: var(--vscode-textLink-foreground); cursor: pointer; text-decoration: none; }
  a.type-chip:hover { text-decoration: underline; }
  span.wc-badge { display: inline-block; font-family: var(--vscode-editor-font-family, monospace); background: var(--vscode-badge-background); color: var(--vscode-badge-foreground); border-radius: 3px; padding: 0 6px; margin: 0 4px 4px 0; font-size: 0.85em; }
  /* A StructuralOnly scalar's predicates: visibly distinct from a re-validated
     badge (outline, not filled) — see wire-render.ts's renderScalarBody doc on
     why an unqualified badge here would misstate the boundary check. */
  span.wc-badge-opaque { background: transparent; border: 1px dashed var(--vscode-descriptionForeground); color: var(--vscode-descriptionForeground); }
  details.wc-type { border: 1px solid var(--vscode-panel-border); border-radius: 4px; margin: 8px 0; padding: 4px 8px; }
  details.wc-type > summary { cursor: pointer; font-family: var(--vscode-editor-font-family, monospace); }
  .wc-shape { white-space: pre-wrap; }
`;

let panel: vscode.WebviewPanel | undefined;

export function registerWireContract(
  context: vscode.ExtensionContext,
  getClient: () => LanguageClient | undefined,
): void {
  context.subscriptions.push(
    vscode.commands.registerCommand(
      "bynk.showWireContract",
      () => void showWireContract(context, getClient),
    ),
  );
}

async function showWireContract(
  context: vscode.ExtensionContext,
  getClient: () => LanguageClient | undefined,
): Promise<void> {
  const client = getClient();
  if (!client) {
    void vscode.window.showErrorMessage(
      "Bynk: Show Wire Contract needs the language server running.",
    );
    return;
  }

  const editor = vscode.window.activeTextEditor;
  if (!editor || editor.document.languageId !== "bynk") {
    void vscode.window.showErrorMessage("Bynk: Show Wire Contract needs a .bynk file open.");
    return;
  }
  const uri = editor.document.uri;
  const position = editor.selection.active;

  const params: WireContractParams = {
    textDocument: { uri: uri.toString() },
    position: { line: position.line, character: position.character },
  };

  let model: WcModel | null;
  try {
    model = await client.sendRequest<WcModel | null>("bynk/wireContract", params);
  } catch (err) {
    void vscode.window.showErrorMessage(`Bynk: Show Wire Contract failed: ${String(err)}`);
    return;
  }
  if (!model) {
    void vscode.window.showInformationMessage(
      "Bynk: place the cursor on a service handler to see its wire contract.",
    );
    return;
  }

  showPanel(context, { model, uri: uri.toString() });
}

function showPanel(context: vscode.ExtensionContext, payload: WcPayload): void {
  if (!panel) {
    panel = vscode.window.createWebviewPanel(
      "bynkWireContract",
      "Bynk Wire Contract",
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
    panel.webview.onDidReceiveMessage((message: WcRevealMessage) => {
      if (message?.type === "reveal") void revealLocation(message.uri, message.range);
    });
  }
  panel.title = `Bynk Wire Contract — ${payload.model.service}`;
  panel.webview.html = renderHtml(panel.webview, context, payload);
  panel.reveal(vscode.ViewColumn.Beside, true);
}

function renderHtml(
  webview: vscode.Webview,
  context: vscode.ExtensionContext,
  payload: WcPayload,
): string {
  const scriptUri = webview.asWebviewUri(
    vscode.Uri.joinPath(context.extensionUri, "out", "wireContract.js"),
  );
  return renderWebviewHtml({
    webview,
    scriptUri,
    nonce: getNonce(),
    title: "Bynk Wire Contract",
    globalName: "__BYNK_WIRE_CONTRACT__",
    payload,
    bodyCss: WIRE_CSS,
  });
}
