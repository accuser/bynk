// Semantic-debugging slice 3: the call stack reads in Bynk (Node).
//
// Exercises the production stack relabel (src/debug.ts): the extension loads the
// `*.bynkdbg.json` sidecar `bynk-emit` emits and rewrites the `stackTrace` response
// so a `.bynk` handler frame is named by its Bynk operation (`GET "/"`) instead of
// the emitted JS function (`http_GET`). Hand-authors a `.ts`+map+sidecar (no compiler)
// so a frame binds to a `.bynk` source; the relabel is the extension's, end to end.

import * as assert from "assert";
import * as path from "path";
import * as os from "os";
import * as fs from "fs";
import { spawn, spawnSync, ChildProcess } from "child_process";
import * as vscode from "vscode";

const B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
function vlq(n: number): string {
  let v = n < 0 ? ((-n) << 1) | 1 : n << 1;
  let out = "";
  do {
    let d = v & 31;
    v >>>= 5;
    if (v) d |= 32;
    out += B64[d];
  } while (v);
  return out;
}

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

describe("Semantic debugging — stack relabel (Node)", () => {
  let dir: string;
  let child: ChildProcess | undefined;

  before(() => {
    dir = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "bynk-stack-")));
    const bynk =
      'context svc\n\nservice api from http {\n  on GET("/") () {\n    let x = 1\n    x\n  }\n}\n';
    fs.writeFileSync(path.join(dir, "svc.bynk"), bynk);
    // Emitted handler. Gen line 2 (`const x`) → svc.bynk:5 (`let x = 1`).
    const ts = [
      "export function http_GET() {", // gen 1
      "  const x = 1;", //              gen 2 -> svc.bynk:5
      "  return x;", //                 gen 3 -> svc.bynk:6
      "}",
      "//# sourceMappingURL=svc.ts.map",
      "",
    ].join("\n");
    fs.writeFileSync(path.join(dir, "svc.ts"), ts);
    const seg = (srcLineDelta: number) => vlq(0) + vlq(0) + vlq(srcLineDelta) + vlq(0);
    const mappings = ["", seg(4), seg(1)].join(";"); // gen2→src4(0-based), gen3→src5
    fs.writeFileSync(
      path.join(dir, "svc.ts.map"),
      JSON.stringify({ version: 3, file: "svc.ts", sources: ["svc.bynk"], sourcesContent: [bynk], names: [], mappings }),
    );
    // The sidecar bynk-emit would write next to the .ts.
    fs.writeFileSync(path.join(dir, "svc.ts.bynkdbg.json"), JSON.stringify({ http_GET: 'GET "/"' }));
    fs.writeFileSync(
      path.join(dir, "entry.ts"),
      'import { http_GET } from "./svc.ts";\nsetInterval(() => http_GET(), 100);\n',
    );
  });

  after(() => {
    child?.kill();
    try {
      fs.rmSync(dir, { recursive: true, force: true });
    } catch {
      /* best-effort */
    }
  });

  it("names a .bynk handler frame by its Bynk operation via the production tracker", async function () {
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

    let frameName: string | undefined;
    const reader = vscode.debug.registerDebugAdapterTrackerFactory("*", {
      createDebugAdapterTracker(s: vscode.DebugSession) {
        return {
          async onDidSendMessage(m: any) {
            if (m.type !== "event" || m.event !== "stopped") return;
            const threadId = m.body?.threadId ?? 0;
            const st: any = await s.customRequest("stackTrace", { threadId, levels: 5 });
            const top = st?.stackFrames?.[0];
            if (!top || !String(top.source?.name ?? "").endsWith("svc.bynk")) {
              void s.customRequest("continue", { threadId });
              return;
            }
            frameName = top.name; // rewritten by the production tracker
            if (verbose) console.log("[frame]", frameName, top.source?.name);
          },
        };
      },
    });

    const bp = new vscode.SourceBreakpoint(
      new vscode.Location(vscode.Uri.file(path.join(dir, "svc.bynk")), new vscode.Position(4, 0)),
    );
    vscode.debug.addBreakpoints([bp]);

    const ok = await vscode.debug.startDebugging(undefined, {
      type: "node",
      request: "attach",
      name: "bynk-stack",
      port,
      cwd: dir, // the sidecar loader scans this
      resolveSourceMapLocations: null,
      skipFiles: ["<node_internals>/**"],
      __bynkChild: "stack-test",
    } as vscode.DebugConfiguration);
    assert.ok(ok, "startDebugging returned true");

    try {
      const deadline = Date.now() + 25_000;
      while (Date.now() < deadline && frameName === undefined) {
        await new Promise((r) => setTimeout(r, 200));
      }
      assert.strictEqual(
        frameName,
        'GET "/"',
        `handler frame named by its Bynk operation, got ${frameName}`,
      );
    } finally {
      reader.dispose();
      vscode.debug.removeBreakpoints([bp]);
      await vscode.debug.stopDebugging();
    }
  });
});
