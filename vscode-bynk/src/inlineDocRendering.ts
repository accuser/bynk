// #849: in-editor doc-comment rendering — the extension's first editor-decoration
// surface. It applies light Markdown affordances (heading colour, bold, italic)
// to `--- … ---` doc blocks *in place*, so a reader gets visual structure while
// reading the source itself — complementary to hover and any separate doc panel.
//
// The Markdown → range tokenizing is the pure `docDecorations` (inlineDoc.ts);
// this file owns the VS Code side: the decoration types, applying them to the
// visible `.bynk` editors, and recomputing on edit / editor-switch / config
// change (debounced, following the same 500 ms pattern testing.ts uses for its
// eager discovery). Clickable `[Name]` links are not here — they come from the
// server's `documentLink` (#848).

import * as vscode from "vscode";

import { docDecorations, type DocDecorationKind } from "./inlineDoc";

const DEBOUNCE_MS = 500;

/** Register the inline doc-rendering surface. Returns nothing; all lifetimes are
 *  tied to `context.subscriptions`. */
export function registerInlineDocRendering(
  context: vscode.ExtensionContext,
): void {
  // [DECISION B] Direct decorations rather than semantic tokens, so bold *looks*
  // bold and italic *looks* italic regardless of the theme. Font size is
  // deliberately never touched — line height stays stable (the no-font-size rule).
  const types: Record<DocDecorationKind, vscode.TextEditorDecorationType> = {
    // Heading colour is a themeable reference (see package.json
    // `contributes.colors`), so it tracks light/dark/high-contrast. Paired with
    // bold so the heading affordance is not colour-only — a non-colour cue for
    // colour-vision-deficient readers, matching the weight cue bold/italic carry.
    heading: vscode.window.createTextEditorDecorationType({
      color: new vscode.ThemeColor("bynk.docHeadingForeground"),
      fontWeight: "bold",
      rangeBehavior: vscode.DecorationRangeBehavior.ClosedClosed,
    }),
    strong: vscode.window.createTextEditorDecorationType({
      fontWeight: "bold",
      rangeBehavior: vscode.DecorationRangeBehavior.ClosedClosed,
    }),
    emphasis: vscode.window.createTextEditorDecorationType({
      fontStyle: "italic",
      rangeBehavior: vscode.DecorationRangeBehavior.ClosedClosed,
    }),
  };
  context.subscriptions.push(types.heading, types.strong, types.emphasis);

  const isEnabled = (): boolean =>
    vscode.workspace
      .getConfiguration("bynk")
      .get<boolean>("inlineDocRendering.enable", true);

  const isBynk = (doc: vscode.TextDocument): boolean =>
    doc.languageId === "bynk";

  // Recompute and apply decorations for one editor. When the feature is off (or
  // the editor isn't a `.bynk` file) we still *set* empty arrays, so toggling the
  // setting off — or switching an editor's language — clears any stale spans.
  const apply = (editor: vscode.TextEditor): void => {
    const enabled = isEnabled() && isBynk(editor.document);
    if (!enabled) {
      for (const type of Object.values(types)) editor.setDecorations(type, []);
      return;
    }
    const buckets: Record<DocDecorationKind, vscode.Range[]> = {
      heading: [],
      strong: [],
      emphasis: [],
    };
    for (const r of docDecorations(editor.document.getText())) {
      buckets[r.kind].push(
        new vscode.Range(r.line, r.startChar, r.line, r.endChar),
      );
    }
    for (const kind of Object.keys(buckets) as DocDecorationKind[]) {
      editor.setDecorations(types[kind], buckets[kind]);
    }
  };

  const applyAllVisible = (): void => {
    for (const editor of vscode.window.visibleTextEditors) apply(editor);
  };

  // Debounce recompute on edits (one pass per edit-burst, no per-keystroke
  // flicker). Editor-switch and config changes apply immediately — those are
  // one-shot, not bursty.
  let timer: ReturnType<typeof setTimeout> | undefined;
  const scheduleForDocument = (doc: vscode.TextDocument): void => {
    if (!isBynk(doc)) return;
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => {
      for (const editor of vscode.window.visibleTextEditors) {
        if (editor.document === doc) apply(editor);
      }
    }, DEBOUNCE_MS);
  };

  context.subscriptions.push(
    vscode.workspace.onDidChangeTextDocument((e) => scheduleForDocument(e.document)),
    // A newly-visible editor (opened, split, or focused) needs its decorations
    // built now — VS Code does not replay them from a prior editor instance.
    vscode.window.onDidChangeVisibleTextEditors(() => applyAllVisible()),
    vscode.window.onDidChangeActiveTextEditor((editor) => {
      if (editor) apply(editor);
    }),
    // The feature toggle and the heading colour both take effect on the next
    // apply; re-decorate everything so flipping the setting is instant.
    vscode.workspace.onDidChangeConfiguration((e) => {
      if (e.affectsConfiguration("bynk.inlineDocRendering")) applyAllVisible();
    }),
  );

  // Decorate whatever is already on screen at activation.
  applyAllVisible();
}
