// Bynk Test Explorer (v0.59).
//
// Runs a project's Bynk tests via `bynkc test --format json` and reports
// pass/fail through the VS Code Test API, with click-through from a failing
// assertion to its `.bynk` source. The extension links no Rust — it shells the
// same `bynkc` the `bynkc: check` task resolves (via `bynk.compilerPath`).
//
// Discovery is **lazy from a run** (proposal v0.59): the tree is (re)built from
// each run's document. A dedicated pre-execution discovery document is a later
// increment.

import { execFile } from "node:child_process";
import * as vscode from "vscode";

import { compilerPath } from "./tasks";

// The `bynkc test --format json` document (mirrors bynkc/src/test_json.rs).
interface JsonLocation {
  path: string;
  line: number;
  col: number;
}
interface JsonCase {
  name: string;
  outcome: "pass" | "fail";
  message?: string;
  location?: JsonLocation;
}
interface JsonSuite {
  name: string;
  kind: "unit" | "integration";
  cases: JsonCase[];
}
interface JsonError {
  kind: "compile" | "runtime";
  message?: string;
  diagnostics?: string[];
  stderr?: string;
}
interface TestRun {
  passed: number;
  failed: number;
  suites?: JsonSuite[];
  error?: JsonError;
}

export function registerTesting(context: vscode.ExtensionContext): void {
  const ctrl = vscode.tests.createTestController("bynk", "Bynk Tests");
  context.subscriptions.push(ctrl);

  // Compile failures surfaced by a run go to the Problems panel, exactly as the
  // `bynkc: check` task does — but in their own collection so a later clean run
  // clears them without disturbing the LSP's live diagnostics.
  const problems = vscode.languages.createDiagnosticCollection("bynk-tests");
  context.subscriptions.push(problems);

  const profile = ctrl.createRunProfile(
    "Run",
    vscode.TestRunProfileKind.Run,
    (request, token) => runHandler(ctrl, problems, request, token),
    true,
  );
  context.subscriptions.push(profile);

  context.subscriptions.push(
    vscode.commands.registerCommand("bynk.runTests", () => {
      void vscode.commands.executeCommand("testing.runAll");
    }),
  );
}

async function runHandler(
  ctrl: vscode.TestController,
  problems: vscode.DiagnosticCollection,
  request: vscode.TestRunRequest,
  token: vscode.CancellationToken,
): Promise<void> {
  const run = ctrl.createTestRun(request);
  problems.clear();

  const root = await findProjectRoot();
  if (!root) {
    run.appendOutput("Bynk: no bynk.toml found in the workspace.\r\n");
    run.end();
    return;
  }

  let doc: TestRun;
  try {
    doc = await runBynkcTest(root, token);
  } catch (e) {
    run.appendOutput(`Bynk: could not run tests — ${String(e)}\r\n`);
    run.end();
    return;
  }

  // A compile failure has no test outcomes: route the diagnostics to the
  // Problems panel (the `bynkc` shape) and stop. These are not test results.
  if (doc.error?.kind === "compile") {
    routeCompileDiagnostics(problems, root, doc.error.diagnostics ?? []);
    run.appendOutput(
      "Bynk: the project did not compile — see the Problems panel.\r\n",
    );
    run.end();
    return;
  }

  for (const suite of doc.suites ?? []) {
    const suiteItem = upsertSuite(ctrl, suite);
    for (const c of suite.cases) {
      const caseItem = upsertCase(ctrl, suiteItem, c);
      run.started(caseItem);
      if (c.outcome === "pass") {
        run.passed(caseItem);
      } else {
        run.failed(caseItem, failureMessage(root, c));
      }
    }
  }

  // A runtime crash: the prefix above already reported; surface the crash as a
  // run-level note (not a `bynkc` diagnostic — it isn't one).
  if (doc.error?.kind === "runtime") {
    run.appendOutput(
      `Bynk: the test runner crashed — ${doc.error.message ?? "unknown error"}\r\n`,
    );
    if (doc.error.stderr) {
      run.appendOutput(doc.error.stderr.replace(/\n/g, "\r\n") + "\r\n");
    }
  }

  run.end();
}

/** Build the `TestMessage` for a failed case, with a `Location` for
 *  click-through when the case carries a `path:line:col`. */
function failureMessage(root: vscode.Uri, c: JsonCase): vscode.TestMessage {
  const msg = new vscode.TestMessage(c.message ?? "test failed");
  if (c.location) {
    const uri = vscode.Uri.joinPath(root, c.location.path);
    // The document's line/col are 1-indexed; VS Code positions are 0-indexed.
    const pos = new vscode.Position(
      Math.max(0, c.location.line - 1),
      Math.max(0, c.location.col - 1),
    );
    msg.location = new vscode.Location(uri, pos);
  }
  return msg;
}

const PREFIX = "bynk-test:";

function upsertSuite(
  ctrl: vscode.TestController,
  suite: JsonSuite,
): vscode.TestItem {
  const id = `${PREFIX}${suite.kind}:${suite.name}`;
  let item = ctrl.items.get(id);
  if (!item) {
    const label = suite.kind === "integration" ? `${suite.name} (integration)` : suite.name;
    item = ctrl.createTestItem(id, label);
    ctrl.items.add(item);
  }
  return item;
}

function upsertCase(
  ctrl: vscode.TestController,
  suiteItem: vscode.TestItem,
  c: JsonCase,
): vscode.TestItem {
  const id = `${suiteItem.id}::${c.name}`;
  let item = suiteItem.children.get(id);
  if (!item) {
    item = ctrl.createTestItem(id, c.name);
    suiteItem.children.add(item);
  }
  return item;
}

/** Route `path:line:col: severity[category]: message` lines (the same shape the
 *  `$bynkc` problem-matcher parses) into the Problems panel. */
function routeCompileDiagnostics(
  problems: vscode.DiagnosticCollection,
  root: vscode.Uri,
  lines: string[],
): void {
  const re = /^(.+?):(\d+):(\d+): (error|warning)\[([^\]]+)\]: (.+)$/;
  const byFile = new Map<string, vscode.Diagnostic[]>();
  for (const line of lines) {
    const m = re.exec(line);
    if (!m) continue;
    const [, file, lineStr, colStr, sev, code, message] = m;
    const uri = vscode.Uri.joinPath(root, file);
    const pos = new vscode.Position(
      Math.max(0, Number(lineStr) - 1),
      Math.max(0, Number(colStr) - 1),
    );
    const diag = new vscode.Diagnostic(
      new vscode.Range(pos, pos),
      message,
      sev === "error"
        ? vscode.DiagnosticSeverity.Error
        : vscode.DiagnosticSeverity.Warning,
    );
    diag.code = code;
    diag.source = "bynkc";
    const key = uri.toString();
    const list = byFile.get(key);
    if (list) list.push(diag);
    else byFile.set(key, [diag]);
  }
  for (const [key, diags] of byFile) {
    problems.set(vscode.Uri.parse(key), diags);
  }
}

/** Run `bynkc test . --format json` at `root` and parse its document. A
 *  non-zero exit is normal (test failures), so we parse stdout regardless and
 *  only reject when there is no parseable document at all. */
function runBynkcTest(
  root: vscode.Uri,
  token: vscode.CancellationToken,
): Promise<TestRun> {
  return new Promise((resolve, reject) => {
    const child = execFile(
      compilerPath(),
      ["test", ".", "--format", "json"],
      { cwd: root.fsPath, maxBuffer: 64 * 1024 * 1024 },
      (_err, stdout, stderr) => {
        const text = stdout.trim();
        if (!text) {
          reject(new Error(stderr.trim() || "no output from `bynkc test`"));
          return;
        }
        try {
          resolve(JSON.parse(text) as TestRun);
        } catch (e) {
          reject(new Error(`could not parse \`bynkc test\` output: ${String(e)}`));
        }
      },
    );
    token.onCancellationRequested(() => child.kill());
  });
}

/** The directory of the nearest `bynk.toml` — walking up from the active
 *  `.bynk` file, then falling back to the workspace-folder roots. Mirrors the
 *  rooting in extension.ts / the LSP's `find_project_root`. */
async function findProjectRoot(): Promise<vscode.Uri | undefined> {
  const exists = async (uri: vscode.Uri): Promise<boolean> => {
    try {
      await vscode.workspace.fs.stat(uri);
      return true;
    } catch {
      return false;
    }
  };

  const active = vscode.window.activeTextEditor?.document;
  if (active?.languageId === "bynk" && active.uri.scheme === "file") {
    let dir = vscode.Uri.joinPath(active.uri, "..");
    for (;;) {
      if (await exists(vscode.Uri.joinPath(dir, "bynk.toml"))) return dir;
      const parent = vscode.Uri.joinPath(dir, "..");
      if (parent.path === dir.path) break;
      dir = parent;
    }
  }
  for (const folder of vscode.workspace.workspaceFolders ?? []) {
    if (await exists(vscode.Uri.joinPath(folder.uri, "bynk.toml"))) {
      return folder.uri;
    }
  }
  return undefined;
}
