// Slice 5 spike: does VS Code's JavaScript debugger render Bynk's tagged ADT
// values in Bynk constructor syntax via `customDescriptionGenerator`?
//
// Bynk's `Result`/`Option`/sum values lower to tagged objects — `Ok(42)` is
// `{ tag: "Ok", value: 42 }`. js-debug's `customDescriptionGenerator` is a
// function (passed as a string) evaluated *in the debuggee* for every object:
// `this` is the object, the first arg is the description js-debug would show
// otherwise. This spike attaches with such a generator, stops at a breakpoint
// where tagged + plain values are in scope, reads the DAP `variables` response,
// and asserts the tagged values read `Ok(42)` / `None` / `BadRequest("oops")` /
// `Ok(Some(42))` while a plain object is left untouched (returns the default).
//
// No compiler needed — the spike isolates the value-formatting mechanism.

import * as assert from "assert";
import * as path from "path";
import * as os from "os";
import * as fs from "fs";
import { spawn, spawnSync, ChildProcess } from "child_process";
import * as vscode from "vscode";

/** True when `node` on PATH is ≥ 22.6 — the floor for `--experimental-strip-types`
 *  (older Node rejects the flag). */
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

// The generator under test is the *shipped* one: read it straight out of
// src/debugValues.ts (the string the provider injects) so the spike can never
// drift from production. The test can't import `src` (its tsconfig roots at
// `test/`), so extract the template-literal value at runtime.
function shippedGenerator(): string {
  const src = fs.readFileSync(
    path.resolve(__dirname, "../../../src/debugValues.ts"),
    "utf8",
  );
  const m = src.match(/BYNK_DESCRIPTION_GENERATOR\s*=\s*`([\s\S]*?)`;/);
  assert.ok(m, "could not extract BYNK_DESCRIPTION_GENERATOR from src/debugValues.ts");
  return m![1];
}
const GENERATOR = shippedGenerator();

describe("Bynk debug values (customDescriptionGenerator)", () => {
  let dir: string;
  let child: ChildProcess | undefined;

  before(() => {
    dir = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "bynk-vals-")));
    // Tagged values (as `bynk-emit`'s runtime emits them) + a plain object, all
    // locals of `tick` so they land in the frame's Local scope. The breakpoint is
    // the `return` line — every value assigned and in scope.
    const entry = [
      "function tick() {",
      '  const ok = { tag: "Ok", value: 42 };',
      '  const none = { tag: "None" };',
      '  const bad = { tag: "BadRequest", message: "oops" };',
      '  const nested = { tag: "Ok", value: { tag: "Some", value: 42 } };',
      '  const plain = { id: 7, name: "x" };',
      "  return [ok, none, bad, nested, plain];", // breakpoint here (line 7, 0-based 6)
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

  it("renders Bynk tagged values in constructor syntax; leaves plain objects alone", async function () {
    if (!nodeStripsTypes()) this.skip(); // needs Node ≥ 22.6 for `--experimental-strip-types`
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

    const bpLine = 7; // the `return` line (1-based)
    const found: Record<string, string> = {};
    const tracker = vscode.debug.registerDebugAdapterTrackerFactory("*", {
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
            // Resume anything that isn't our breakpoint line (the --inspect-brk
            // entry pause, internals, …).
            if (!frame || frame.line !== bpLine) {
              void s.customRequest("continue", { threadId });
              return;
            }
            try {
              const sc: any = await s.customRequest("scopes", { frameId: frame.id });
              const local =
                sc.scopes.find((x: any) => x.presentationHint === "locals") ?? sc.scopes[0];
              const vars: any = await s.customRequest("variables", {
                variablesReference: local.variablesReference,
              });
              for (const v of vars.variables) {
                found[v.name] = v.value;
                if (verbose) console.log("[var]", v.name, "=>", v.value);
              }
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

    const ok = await vscode.debug.startDebugging(undefined, {
      type: "node",
      request: "attach",
      name: "bynk-values-spike",
      port,
      resolveSourceMapLocations: null,
      skipFiles: ["<node_internals>/**"],
      customDescriptionGenerator: GENERATOR,
    });
    assert.ok(ok, "startDebugging returned true");

    try {
      const deadline = Date.now() + 25_000;
      while (Date.now() < deadline && Object.keys(found).length < 5) {
        await new Promise((r) => setTimeout(r, 200));
      }
      if (verbose) console.log("[found]", JSON.stringify(found, null, 2));
      // The headline: tagged values read in Bynk constructor syntax.
      assert.strictEqual(found.ok, "Ok(42)", "Result Ok renders as Ok(42)");
      assert.strictEqual(found.none, "None", "nullary variant renders as None");
      assert.strictEqual(found.bad, 'BadRequest("oops")', "variant with a field");
      assert.strictEqual(found.nested, "Ok(Some(42))", "nested ADTs render inline");
      // A plain (non-tagged) object is left to the default description.
      assert.ok(
        found.plain !== undefined && !/^[A-Z]\w*\(/.test(found.plain),
        `plain object untouched, got ${found.plain}`,
      );
    } finally {
      tracker.dispose();
      vscode.debug.removeBreakpoints([bp]);
      await vscode.debug.stopDebugging();
    }
  });
});
