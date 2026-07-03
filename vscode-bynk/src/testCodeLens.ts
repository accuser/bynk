// v0.78: a Test CodeLens — a `▷ Run | Debug` line above each test case in a `.bynk`
// file, the explicit, discoverable counterpart to VS Code's native gutter glyphs (which
// eager discovery in testing.ts also lights up). This is a *client-side*
// `CodeLensProvider`, distinct from the LSP CodeLens middleware in codelens.ts (which
// hydrates the server's `N references` lenses) — so a referenced test shows both lenses.
//
// It places lenses from the Test Explorer's discovered case locations (testing.ts's
// `testCases`), and each lens carries its case name to the per-case commands
// `bynk.runTestCase` / `bynk.debugTestCase` (v0.127): `▷ Run Test` runs *that*
// case (via `bynkc test --case`), not the whole project.

import * as vscode from "vscode";

import type { TestApi } from "./testing";

function enabled(): boolean {
  return vscode.workspace.getConfiguration("bynk").get<boolean>("testCodeLens", true);
}

class BynkTestCodeLensProvider implements vscode.CodeLensProvider {
  private readonly emitter = new vscode.EventEmitter<void>();
  readonly onDidChangeCodeLenses = this.emitter.event;

  constructor(private readonly api: TestApi) {
    // Refresh after a discovery settles, and when the toggle flips (so it takes
    // effect without reopening the file).
    api.onDidChangeTests(() => this.emitter.fire());
    vscode.workspace.onDidChangeConfiguration((e) => {
      if (e.affectsConfiguration("bynk.testCodeLens")) this.emitter.fire();
    });
  }

  provideCodeLenses(document: vscode.TextDocument): vscode.CodeLens[] {
    if (!enabled()) return [];
    const lenses: vscode.CodeLens[] = [];
    for (const { range, name } of this.api.testCases(document.uri)) {
      lenses.push(
        new vscode.CodeLens(range, {
          title: "$(run) Run Test",
          tooltip: "Run this test case",
          command: "bynk.runTestCase",
          arguments: [name],
        }),
        new vscode.CodeLens(range, {
          title: "$(debug-alt-small) Debug Test",
          tooltip: "Debug this case under the inspector — your breakpoint pauses",
          command: "bynk.debugTestCase",
          arguments: [name],
        }),
      );
    }
    return lenses;
  }

  dispose(): void {
    this.emitter.dispose();
  }
}

/** Register the `.bynk` test CodeLens provider, fed by the Test Explorer's discovery. */
export function registerTestCodeLens(context: vscode.ExtensionContext, api: TestApi): void {
  const provider = new BynkTestCodeLensProvider(api);
  context.subscriptions.push(
    provider,
    vscode.languages.registerCodeLensProvider({ language: "bynk" }, provider),
  );
}
