// Semantic-debugging slice 1: the value interposer end to end (Node).
//
// Exercises the *production* rewrite: a `__bynkChild` attach (the marker the
// provider stamps) with NO in-debuggee `customDescriptionGenerator`, so the
// extension's registered tracker (src/debug.ts → renderBynkValue) rewrites the raw
// `variables` previews editor-side. Asserts tagged values read in Bynk syntax and a
// plain object is untouched — and surfaces how deep js-debug's preview lets us
// render a nested ADT (the slice's depth question, which decides the slice-5
// generator's fate). No compiler needed.

import * as assert from "assert";
import * as path from "path";
import * as os from "os";
import * as fs from "fs";
import { spawn, spawnSync, ChildProcess } from "child_process";
import * as vscode from "vscode";

function nodeStripsTypes(): boolean {
  try {
    const v = spawnSync("node", ["--version"], { encoding: "utf8" }).stdout.trim();
    const m = v.match(/^v(\d+)\.(\d+)/);
    if (!m) return false;
    const [maj, min] = [Number(m[1]), Number(m[2])];
    return maj > 22 || (maj === 22 && min >= 6);
  } catch {
    return false;
  }
}

describe("Semantic debugging — value interposer (Node)", () => {
  let dir: string;
  let child: ChildProcess | undefined;

  before(() => {
    dir = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "bynk-semvals-")));
    const entry = [
      "function tick() {",
      '  const ok = { tag: "Ok", value: 42 };',
      '  const none = { tag: "None" };',
      '  const bad = { tag: "BadRequest", message: "oops" };',
      '  const nested = { tag: "Ok", value: { tag: "Some", value: 42 } };',
      '  const plain = { id: 7, name: "x" };',
      "  return [ok, none, bad, nested, plain];", // breakpoint (line 7)
      "}",
      "setInterval(tick, 100);",
      "",
    ].join("\n");
    fs.writeFileSync(path.join(dir, "entry.ts"), entry);
  });

  after(() => {
    child?.kill();
    try {
      fs.rmSync(dir, { recursive: true, force: true });
    } catch {
      /* best-effort */
    }
  });

  it("rewrites tagged-value previews into Bynk syntax via the production tracker", async function () {
    if (!nodeStripsTypes()) this.skip();
    this.timeout(40_000);
    const verbose = process.env.BYNK_DEBUG_SPIKE === "verbose";

    child = spawn(
      "node",
      ["--experimental-strip-types", "--inspect-brk=0", path.join(dir, "entry.ts")],
      { stdio: ["ignore", "ignore", "pipe"] },
    );
    const port = await new Promise<number>((resolve, reject) => {
      const t = setTimeout(() => reject(new Error("node never printed an inspector URL")), 8000);
      child!.stderr?.on("data", (b) => {
        const m = String(b).match(/ws:\/\/127\.0\.0\.1:(\d+)\//);
        if (m) {
          clearTimeout(t);
          resolve(Number(m[1]));
        }
      });
    });

    const bpLine = 7;
    const found: Record<string, string> = {};
    const reader = vscode.debug.registerDebugAdapterTrackerFactory("*", {
      createDebugAdapterTracker(s: vscode.DebugSession) {
        return {
          async onDidSendMessage(m: any) {
            if (m.type !== "event" || m.event !== "stopped") return;
            const threadId = m.body?.threadId ?? 0;
            let frame: any;
            try {
              const st: any = await s.customRequest("stackTrace", { threadId, levels: 1 });
              frame = st?.stackFrames?.[0];
            } catch {
              /* ignore */
            }
            if (!frame || frame.line !== bpLine) {
              void s.customRequest("continue", { threadId });
              return;
            }
            try {
              const sc: any = await s.customRequest("scopes", { frameId: frame.id });
              const local =
                sc.scopes.find((x: any) => x.presentationHint === "locals") ?? sc.scopes[0];
              // This response is rewritten by the production tracker (it carries our
              // `__bynkChild` marker), so the values we read are the rendered ones.
              const vars: any = await s.customRequest("variables", {
                variablesReference: local.variablesReference,
              });
              for (const v of vars.variables) found[v.name] = v.value;
              if (verbose) console.log("[found]", JSON.stringify(found, null, 2));
            } catch (e) {
              if (verbose) console.log("[err]", String(e));
            }
          },
        };
      },
    });

    const bp = new vscode.SourceBreakpoint(
      new vscode.Location(
        vscode.Uri.file(path.join(dir, "entry.ts")),
        new vscode.Position(bpLine - 1, 0),
      ),
    );
    vscode.debug.addBreakpoints([bp]);

    // The production marker — no `customDescriptionGenerator`. The rewrite is the
    // extension's editor-side tracker.
    const ok = await vscode.debug.startDebugging(undefined, {
      type: "node",
      request: "attach",
      name: "bynk-semvals",
      port,
      resolveSourceMapLocations: null,
      skipFiles: ["<node_internals>/**"],
      __bynkChild: "semvals-test",
    } as vscode.DebugConfiguration);
    assert.ok(ok, "startDebugging returned true");

    try {
      const deadline = Date.now() + 25_000;
      while (Date.now() < deadline && Object.keys(found).length < 5) {
        await new Promise((r) => setTimeout(r, 200));
      }
      assert.strictEqual(found.ok, "Ok(42)", "Result Ok");
      assert.strictEqual(found.none, "None", "nullary variant");
      assert.strictEqual(found.bad, 'BadRequest("oops")', "variant with a field");
      // Depth: a nested ADT renders as deep as js-debug's preview carries. We assert
      // the constructor is named and the inner value is at least referenced.
      assert.ok(
        /^Ok\(/.test(found.nested),
        `nested ADT renders as Ok(…), got ${found.nested}`,
      );
      // A plain (non-tagged) object is left exactly as js-debug rendered it.
      assert.ok(
        found.plain !== undefined && !/^[A-Z]\w*\(/.test(found.plain),
        `plain object untouched, got ${found.plain}`,
      );
    } finally {
      reader.dispose();
      vscode.debug.removeBreakpoints([bp]);
      await vscode.debug.stopDebugging();
    }
  });
});
