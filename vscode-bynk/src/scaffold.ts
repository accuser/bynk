// Scaffolding commands: "Bynk: New Context" and "Bynk: New Project".
//
// Both write files via `workspace.fs` (no external deps) and refuse to clobber
// anything that already exists, then open what they created. They lower the
// first-five-minutes friction the LSP features assume a project for.

import * as vscode from "vscode";

/** A dotted Bynk unit name: `commerce.orders`, `shop`, … */
const UNIT_NAME = /^[a-z][a-zA-Z0-9]*(\.[a-z][a-zA-Z0-9]*)*$/;

/** `commerce.orders` → `orders` (the file's base name). */
function lastSegment(name: string): string {
  const parts = name.split(".");
  return parts[parts.length - 1];
}

async function exists(uri: vscode.Uri): Promise<boolean> {
  try {
    await vscode.workspace.fs.stat(uri);
    return true;
  } catch {
    return false;
  }
}

async function writeUtf8(uri: vscode.Uri, text: string): Promise<void> {
  await vscode.workspace.fs.writeFile(uri, Buffer.from(text, "utf8"));
}

/** The folder new files land in: the workspace's `src/` if present, else the
 *  workspace root. Undefined when there is no workspace folder. */
async function targetDir(): Promise<vscode.Uri | undefined> {
  const folder = vscode.workspace.workspaceFolders?.[0];
  if (!folder) return undefined;
  const src = vscode.Uri.joinPath(folder.uri, "src");
  return (await exists(src)) ? src : folder.uri;
}

/** Bynk: New Context — prompt for a dotted name, write a `context` skeleton. */
export async function newContext(): Promise<void> {
  const dir = await targetDir();
  if (!dir) {
    void vscode.window.showErrorMessage(
      "Bynk: open a folder before creating a context.",
    );
    return;
  }
  const name = await vscode.window.showInputBox({
    title: "New Bynk context",
    prompt: "Context name (dotted, e.g. commerce.orders)",
    placeHolder: "commerce.orders",
    validateInput: (v) =>
      UNIT_NAME.test(v.trim()) ? undefined : "Use a dotted lowercase name like `commerce.orders`.",
  });
  if (!name) return; // cancelled
  const trimmed = name.trim();

  const file = vscode.Uri.joinPath(dir, `${lastSegment(trimmed)}.bynk`);
  if (await exists(file)) {
    void vscode.window.showErrorMessage(
      `Bynk: ${vscode.workspace.asRelativePath(file)} already exists.`,
    );
    return;
  }

  await writeUtf8(file, `context ${trimmed}\n\n`);
  const doc = await vscode.workspace.openTextDocument(file);
  await vscode.window.showTextDocument(doc);
}

/** Bynk: New Project — scaffold `bynk.toml` + `src/<name>.bynk` with a starter
 *  context, in a chosen folder. */
export async function newProject(): Promise<void> {
  const folder = vscode.workspace.workspaceFolders?.[0];
  if (!folder) {
    void vscode.window.showErrorMessage(
      "Bynk: open a folder before creating a project.",
    );
    return;
  }

  const name = await vscode.window.showInputBox({
    title: "New Bynk project",
    prompt: "Project name (a starter context of the same name is created)",
    placeHolder: "my-app",
    validateInput: (v) =>
      /^[a-z][a-z0-9-]*$/.test(v.trim())
        ? undefined
        : "Use a lowercase kebab-case name like `my-app`.",
  });
  if (!name) return;
  const project = name.trim();
  const unit = project.replace(/-/g, "");

  const toml = vscode.Uri.joinPath(folder.uri, "bynk.toml");
  if (await exists(toml)) {
    void vscode.window.showErrorMessage(
      "Bynk: this folder already has a bynk.toml.",
    );
    return;
  }

  await writeUtf8(
    toml,
    `[project]\nname = "${project}"\nversion = "0.1.0"\n\n[paths]\nsrc = "src"\ntests = "tests"\nout = "out"\n`,
  );
  const context = vscode.Uri.joinPath(folder.uri, "src", `${unit}.bynk`);
  await writeUtf8(context, `context ${unit}\n\n`);

  const doc = await vscode.workspace.openTextDocument(context);
  await vscode.window.showTextDocument(doc);
  void vscode.window.showInformationMessage(
    `Bynk: created project “${project}”.`,
  );
}
