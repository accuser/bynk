// Semantic-debugging track, slice 0 — the load-bearing spike (interposition
// ladder, rung 1): can a `DebugAdapterTracker` *mutate* a DAP response in flight,
// so the consumer sees the rewritten value?
//
// VS Code's tracker is documented as observe-only. If mutating the message object
// in `onDidSendMessage` nonetheless changes what the response delivers, the whole
// semantic layer drops in as a one-file editor-side rewrite — no wrapping proxy
// adapter, runtime-agnostic (works on workerd, unlike slice 5's in-debuggee
// generator). This spike rewrites a known local's `value` to a sentinel and reads
// it back via the DAP; if the sentinel comes through, rung 1 is viable.
//
// No compiler needed — it isolates the interposition mechanism.

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

const SENTINEL = "Ok(42) «rewritten-by-tracker»";

describe("Semantic debugging — interposition rung 1 (tracker mutation)", () => {
  let dir: string;
  let child: ChildProcess | undefined;

  before(() => {
    dir = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "bynk-semdbg-")));
    const entry = [
      "function tick() {",
      '  const ok = { tag: "Ok", value: 42 };',
      '  const plain = { id: 7 };',
      "  return [ok, plain];", // breakpoint here (line 4, 0-based 3)
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

  it("a tracker rewrite of a `variables` response reaches the consumer", async function () {
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

    const bpLine = 4;
    let okValue: string | undefined;
    let rewroteResponses = 0;

    const tracker = vscode.debug.registerDebugAdapterTrackerFactory("*", {
      createDebugAdapterTracker(s: vscode.DebugSession) {
        return {
          // The interposition under test: mutate the response object in place.
          onDidSendMessage(m: any) {
            if (m?.type === "response" && m.command === "variables" && m.body?.variables) {
              for (const v of m.body.variables) {
                if (v.name === "ok") {
                  v.value = SENTINEL;
                  rewroteResponses++;
                }
              }
            }
            // Drive the session: resume anything that isn't our breakpoint line,
            // and at the breakpoint read the locals (whose response we rewrite above).
            if (m?.type === "event" && m.event === "stopped") {
              void (async () => {
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
                  const vars: any = await s.customRequest("variables", {
                    variablesReference: local.variablesReference,
                  });
                  const ok = vars.variables.find((x: any) => x.name === "ok");
                  okValue = ok?.value;
                  if (verbose) console.log("[spike] okValue =>", okValue, "rewrites=", rewroteResponses);
                } catch (e) {
                  if (verbose) console.log("[err]", String(e));
                }
              })();
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
      name: "bynk-semdbg-spike",
      port,
      resolveSourceMapLocations: null,
      skipFiles: ["<node_internals>/**"],
    });
    assert.ok(ok, "startDebugging returned true");

    try {
      const deadline = Date.now() + 25_000;
      while (Date.now() < deadline && okValue === undefined) {
        await new Promise((r) => setTimeout(r, 200));
      }
      // The verdict. If the consumer sees the sentinel, a tracker can rewrite DAP
      // responses (rung 1 viable). If it sees the raw object preview, it cannot,
      // and the track climbs to rung 2 (a wrapping proxy adapter).
      assert.strictEqual(
        okValue,
        SENTINEL,
        `tracker mutation did NOT propagate — saw ${okValue}. Rung 1 is out; try rung 2.`,
      );
    } finally {
      tracker.dispose();
      vscode.debug.removeBreakpoints([bp]);
      await vscode.debug.stopDebugging();
    }
  });
});
