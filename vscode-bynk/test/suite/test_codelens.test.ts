// v0.78: the Test CodeLens + eager discovery, end to end. Opening a `.bynk` test file
// (without opening the Testing view) should trigger discovery and surface a
// `Run Test | Debug Test` CodeLens at the case declaration. Guarded on `bynkc`
// (discovery is a `bynkc test --no-run` compile); the harness provisions it.

import * as assert from "assert";
import * as path from "path";
import * as fs from "fs";
import { spawnSync } from "child_process";
import * as vscode from "vscode";

function have(tool: string): boolean {
  try {
    return spawnSync(tool, ["--version"], { stdio: "ignore" }).status === 0;
  } catch {
    return false;
  }
}

describe("Test CodeLens (eager discovery)", () => {
  let dir: string;

  after(() => {
    if (dir) {
      try {
        fs.rmSync(dir, { recursive: true, force: true });
      } catch {
        /* best-effort */
      }
    }
  });

  it("shows Run/Debug lenses at a test case after opening the file", async function () {
    if (!have("bynkc")) this.skip();
    this.timeout(60_000);

    // A project inside the workspace folder with one test case.
    const workspace = path.resolve(__dirname, "../../../test/fixtures/workspace");
    dir = fs.realpathSync(fs.mkdtempSync(path.join(workspace, ".cl-")));
    fs.mkdirSync(path.join(dir, "src"));
    fs.mkdirSync(path.join(dir, "tests"));
    fs.writeFileSync(
      path.join(dir, "bynk.toml"),
      "[project]\nname = \"cl\"\n\n[paths]\nsrc = \"src\"\ntests = \"tests\"\n",
    );
    fs.writeFileSync(
      path.join(dir, "src", "calc.bynk"),
      "commons calc {\n  fn dbl(n: Int) -> Int { n + n }\n}\n",
    );
    const testSrc = "test calc {\n  test \"doubles\" {\n    assert dbl(3) == 6\n  }\n}\n";
    fs.writeFileSync(path.join(dir, "tests", "calc.bynk"), testSrc);
    const caseLine = testSrc.split("\n").findIndex((l) => l.includes('test "doubles"'));

    // Open the test file — this is what should trigger eager discovery (no Testing
    // view interaction).
    const uri = vscode.Uri.file(path.join(dir, "tests", "calc.bynk"));
    await vscode.window.showTextDocument(await vscode.workspace.openTextDocument(uri));

    // Poll the CodeLens provider until discovery settles and lenses appear.
    let lenses: vscode.CodeLens[] = [];
    const deadline = Date.now() + 45_000;
    while (Date.now() < deadline) {
      lenses = (await vscode.commands.executeCommand<vscode.CodeLens[]>(
        "vscode.executeCodeLensProvider",
        uri,
      )) ?? [];
      // Only our test lenses (ignore any LSP reference lenses).
      const ours = lenses.filter((l) =>
        ["bynk.runTests", "bynk.debugTests"].includes(l.command?.command ?? ""),
      );
      if (ours.length >= 2) {
        lenses = ours;
        break;
      }
      await new Promise((r) => setTimeout(r, 500));
    }

    const commands = lenses.map((l) => l.command?.command);
    assert.ok(commands.includes("bynk.runTests"), `Run Test lens present, got ${commands}`);
    assert.ok(commands.includes("bynk.debugTests"), `Debug Test lens present, got ${commands}`);
    // Anchored at the case declaration line.
    assert.ok(
      lenses.every((l) => l.range.start.line === caseLine),
      `lenses at the case line ${caseLine}, got ${lenses.map((l) => l.range.start.line)}`,
    );
  });
});
