// CodeLens middleware: hydrate the server's `showReferences` command arguments.
//
// The reference-count CodeLens (`N references`) carries the built-in
// `editor.action.showReferences` command, but its arguments arrive from the
// language server as plain LSP JSON (a uri string, a `{line, character}`, an
// array of `{uri, range}`). That command validates its arguments with
// `instanceof`, so the plain objects are rejected with "argument does not match
// one of these constraints…". The language client passes command arguments
// through verbatim, so we re-instantiate them here, on the client.

import * as vscode from "vscode";
import type { ProvideCodeLensesSignature } from "vscode-languageclient/node";

const SHOW_REFERENCES = "editor.action.showReferences";

function toPosition(p: unknown): vscode.Position {
  if (p instanceof vscode.Position) return p;
  const { line, character } = p as { line: number; character: number };
  return new vscode.Position(line, character);
}

function toRange(r: unknown): vscode.Range {
  if (r instanceof vscode.Range) return r;
  const { start, end } = r as { start: unknown; end: unknown };
  return new vscode.Range(toPosition(start), toPosition(end));
}

function toUri(u: unknown): vscode.Uri {
  return u instanceof vscode.Uri ? u : vscode.Uri.parse(u as string);
}

function toLocation(l: unknown): vscode.Location {
  if (l instanceof vscode.Location) return l;
  const { uri, range } = l as { uri: unknown; range: unknown };
  return new vscode.Location(toUri(uri), toRange(range));
}

/** Rewrite a `showReferences` lens's `[uri, position, locations]` arguments
 *  into real `vscode.Uri` / `Position` / `Location[]` instances. Idempotent and
 *  defensive — anything that isn't the expected three-argument shape is left
 *  untouched. */
function hydrate(lens: vscode.CodeLens): void {
  const cmd = lens.command;
  if (cmd?.command !== SHOW_REFERENCES || !Array.isArray(cmd.arguments)) return;
  if (cmd.arguments.length !== 3) return;
  const [uri, position, locations] = cmd.arguments;
  if (!Array.isArray(locations)) return;
  cmd.arguments = [toUri(uri), toPosition(position), locations.map(toLocation)];
}

/** `provideCodeLenses` middleware: hydrate the reference-count lenses after the
 *  server returns them. */
export async function provideCodeLenses(
  document: vscode.TextDocument,
  token: vscode.CancellationToken,
  next: ProvideCodeLensesSignature,
): Promise<vscode.CodeLens[] | null | undefined> {
  const lenses = await next(document, token);
  lenses?.forEach(hydrate);
  return lenses;
}
