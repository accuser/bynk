// #847: "Bynk: Show Documentation" — the documentation webview panel, the
// second consumer of the shared webview substrate (`webviewHost.ts`) #846
// introduced. Issues the `bynk/documentationModel` custom LSP request for the
// active file and renders every declaration as a reference page.
//
// Simpler than #846's sequence view in one way — the request carries no cursor
// position (the page is the whole file, Decision A) — and richer in another: it
// also handles an `openExternal` message, since rendered doc-comment Markdown
// can contain links (gated to http(s) by `webviewHost.openExternalHref`).

import * as vscode from "vscode";
import type { LanguageClient } from "vscode-languageclient/node";

import {
  getNonce,
  openExternalHref,
  renderWebviewHtml,
  revealLocation,
  type WebviewMessage,
} from "./webviewHost";
import type { DocModel, DocPayload } from "./webview/docTypes";

interface DocumentationModelParams {
  textDocument: { uri: string };
}

// The doc page's own styling, layered over the shared base in `renderWebviewHtml`.
const DOC_CSS = `
  header { border-bottom: 1px solid var(--vscode-panel-border); margin-bottom: 12px; padding-bottom: 8px; }
  .kind { color: var(--vscode-descriptionForeground); font-weight: normal; font-size: 0.75em; text-transform: uppercase; letter-spacing: 0.05em; margin-right: 8px; }
  .name { font-family: var(--vscode-editor-font-family, monospace); }
  h1, h2, h3, h4 { cursor: pointer; }
  .entry { border-top: 1px solid var(--vscode-panel-border); padding-top: 4px; }
  .body pre { background: var(--vscode-textCodeBlock-background); padding: 8px; overflow-x: auto; border-radius: 3px; }
  .body code { font-family: var(--vscode-editor-font-family, monospace); }
  .no-doc { color: var(--vscode-descriptionForeground); font-style: italic; }
  .toggle { display: inline-flex; align-items: center; gap: 4px; font-size: 0.85em; color: var(--vscode-descriptionForeground); }
  #root[data-hide-undocumented="true"] .entry[data-documented="false"] { display: none; }
`;

let panel: vscode.WebviewPanel | undefined;

export function registerDocumentationView(
  context: vscode.ExtensionContext,
  getClient: () => LanguageClient | undefined,
): void {
  context.subscriptions.push(
    vscode.commands.registerCommand(
      "bynk.showDocumentation",
      () => void showDocumentation(context, getClient),
    ),
  );
}

async function showDocumentation(
  context: vscode.ExtensionContext,
  getClient: () => LanguageClient | undefined,
): Promise<void> {
  const client = getClient();
  if (!client) {
    void vscode.window.showErrorMessage(
      "Bynk: Show Documentation needs the language server running.",
    );
    return;
  }

  const editor = vscode.window.activeTextEditor;
  if (!editor || editor.document.languageId !== "bynk") {
    void vscode.window.showErrorMessage("Bynk: Show Documentation needs a .bynk file open.");
    return;
  }
  const uri = editor.document.uri;

  const params: DocumentationModelParams = { textDocument: { uri: uri.toString() } };
  let model: DocModel | null;
  try {
    model = await client.sendRequest<DocModel | null>("bynk/documentationModel", params);
  } catch (err) {
    void vscode.window.showErrorMessage(`Bynk: Show Documentation failed: ${String(err)}`);
    return;
  }
  if (!model) {
    void vscode.window.showInformationMessage(
      "Bynk: no documentation for this file (not a project file, or no committed analysis yet).",
    );
    return;
  }

  showPanel(context, { model, uri: uri.toString() });
}

function showPanel(context: vscode.ExtensionContext, payload: DocPayload): void {
  if (!panel) {
    panel = vscode.window.createWebviewPanel(
      "bynkDocumentation",
      "Bynk Documentation",
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
    panel.webview.onDidReceiveMessage((message: WebviewMessage) => {
      if (message?.type === "reveal") void revealLocation(message.uri, message.range);
      else if (message?.type === "openExternal") void openExternalHref(message.href);
    });
  }
  panel.title = `Bynk Documentation — ${payload.model.unitName}`;
  panel.webview.html = renderHtml(panel.webview, context, payload);
  panel.reveal(vscode.ViewColumn.Beside, true);
}

function renderHtml(
  webview: vscode.Webview,
  context: vscode.ExtensionContext,
  payload: DocPayload,
): string {
  const scriptUri = webview.asWebviewUri(
    vscode.Uri.joinPath(context.extensionUri, "out", "docview.js"),
  );
  return renderWebviewHtml({
    webview,
    scriptUri,
    nonce: getNonce(),
    title: "Bynk Documentation",
    globalName: "__BYNK_DOC_MODEL__",
    payload,
    bodyCss: DOC_CSS,
  });
}
