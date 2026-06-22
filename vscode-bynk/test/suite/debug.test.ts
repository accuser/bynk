// Slice 4 spike + integration test: does VS Code's built-in JavaScript debugger
// (`pwa-node`) attach to a `node --inspect-brk` process running emitted `.ts` and
// bind a breakpoint set in the originating `.bynk` source — resolving it through
// the `.ts.map`? This is the load-bearing question for the one-click debug UX: the
// `DebugConfigurationProvider` will produce exactly this kind of `pwa-node` attach.
//
// It hand-authors a tiny `.ts` + `.ts.map` + `.bynk` (no compiler needed) so the
// test isolates the attach mechanics, runs it under `node --inspect-brk`, attaches,
// sets a breakpoint in the `.bynk`, and watches the debug-adapter protocol for the
// breakpoint to *bind and pause*.

import * as assert from "assert";
import * as path from "path";
import * as os from "os";
import * as fs from "fs";
import { spawn, ChildProcess } from "child_process";
import * as vscode from "vscode";

// Minimal base64-VLQ encoder for a source-map v3 `mappings` string.
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

describe("Bynk debug attach", () => {
  let dir: string;
  let child: ChildProcess | undefined;

  before(() => {
    // realpath so the breakpoint URI matches the path the debugger resolves the
    // loaded source to (on macOS `/var/folders/...` is a symlink to `/private/...`).
    dir = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "bynk-dbg-")));
    // The `.bynk` "source" (only its line structure + content matter here).
    const bynk = "commons calc {\n  fn dbl(n: Int) -> Int {\n    let r = n + n\n    r\n  }\n}\n";
    fs.writeFileSync(path.join(dir, "calc.bynk"), bynk);

    // The "emitted" TS. Line 2 (`const r = n + n;`) maps back to calc.bynk line 3.
    const ts = [
      "export function dbl(n) {", // gen line 1 (0-based 0)
      "  const r = n + n;", //        gen line 2 -> calc.bynk:3
      "  return r;", //               gen line 3 -> calc.bynk:4
      "}",
      "//# sourceMappingURL=calc.ts.map",
      "",
    ].join("\n");
    fs.writeFileSync(path.join(dir, "calc.ts"), ts);

    // Hand-authored map: gen line 2 (0-based 1) -> calc.bynk line 3 (0-based 2);
    // gen line 3 -> calc.bynk line 4. One segment per generated line, col 0.
    // Segment fields are deltas: [genCol, srcIdx, srcLineDelta, srcCol].
    const seg = (srcLineDelta: number) => vlq(0) + vlq(0) + vlq(srcLineDelta) + vlq(0);
    const mappings = [
      "", //                 gen line 1: unmapped
      seg(2), //             gen line 2 -> src line 2 (0-based) = calc.bynk:3
      seg(1), //             gen line 3 -> src line 3 = calc.bynk:4
    ].join(";");
    const map = {
      version: 3,
      file: "calc.ts",
      sources: ["calc.bynk"],
      sourcesContent: [bynk],
      names: [],
      mappings,
    };
    fs.writeFileSync(path.join(dir, "calc.ts.map"), JSON.stringify(map));

    // Entry: call dbl on a short interval so it keeps hitting the breakpoint.
    const entry = [
      'import { dbl } from "./calc.ts";',
      "setInterval(() => { dbl(3); }, 200);",
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

  it("a breakpoint in .bynk binds and pauses under a pwa-node attach", async function () {
    this.timeout(40_000);

    // Launch the emitted TS under the inspector (type-stripping; Node >= 22.6).
    // `--inspect-brk=0` lets Node pick a free port, avoiding collisions.
    child = spawn(
      "node",
      ["--experimental-strip-types", "--inspect-brk=0", path.join(dir, "entry.ts")],
      { stdio: ["ignore", "ignore", "pipe"] },
    );
    // Wait for node to print its inspector URL, and capture the chosen port.
    const port = await new Promise<number>((resolve, reject) => {
      const t = setTimeout(() => reject(new Error("node never printed an inspector URL")), 8000);
      child!.stderr?.on("data", (b) => {
        const s = String(b);
        if (process.env.BYNK_DEBUG_SPIKE === "verbose") console.log("[node]", s.trim());
        const m = s.match(/ws:\/\/127\.0\.0\.1:(\d+)\//);
        if (m) {
          clearTimeout(t);
          resolve(Number(m[1]));
        }
      });
    });

    // Watch the debug-adapter protocol. `--inspect-brk` pauses at entry; we
    // continue past any non-breakpoint stop. A `stopped` with reason `breakpoint`
    // is the proof: the breakpoint set in `.bynk` bound (reverse-mapped through
    // the `.ts.map`) and execution paused on it.
    let hitBreakpoint = false;
    const tracker = vscode.debug.registerDebugAdapterTrackerFactory("*", {
      createDebugAdapterTracker(s: vscode.DebugSession) {
        return {
          async onDidSendMessage(m: any) {
            if (m.type !== "event" || m.event !== "stopped") return;
            const threadId = m.body?.threadId ?? 0;
            // Where did it stop? `--inspect-brk` pauses at entry; a source-mapped
            // breakpoint hit can be reported with reason `step`, so decide by the
            // top frame's source, not the reason.
            let frame: any;
            try {
              const st: any = await s.customRequest("stackTrace", { threadId, levels: 1 });
              frame = st?.stackFrames?.[0];
            } catch {
              /* ignore */
            }
            const name: string = frame?.source?.name ?? "";
            if (process.env.BYNK_DEBUG_SPIKE === "verbose") {
              console.log("[stop]", m.body?.reason, name, frame?.line);
            }
            if (name.endsWith("calc.bynk") || name.endsWith("calc.ts")) {
              // Paused in our code — the `.bynk` breakpoint bound (reverse-mapped
              // through the `.ts.map`) and execution paused on it.
              hitBreakpoint = true;
            } else {
              // Entry / internals — resume.
              void s.customRequest("continue", { threadId });
            }
          },
        };
      },
    });

    // Set a breakpoint in the `.bynk` at line 3 (`let r = n + n`).
    const bynkUri = vscode.Uri.file(path.join(dir, "calc.bynk"));
    const bp = new vscode.SourceBreakpoint(
      new vscode.Location(bynkUri, new vscode.Position(2, 0)),
    );
    vscode.debug.addBreakpoints([bp]);

    // Attach VS Code's built-in JS debugger (the config the provider will produce).
    const ok = await vscode.debug.startDebugging(undefined, {
      type: "node",
      request: "attach",
      name: "bynk-spike",
      port,
      // Honour source maps wherever the emitted output lives (the default limits
      // resolution to the workspace folder; our output may be in a build dir or,
      // here, a temp dir). The real provider sets this for the same reason.
      resolveSourceMapLocations: null,
      // Skip the toolchain machinery, per ADR 0103 D5 (not exercised here, but the
      // shape the provider uses).
      skipFiles: ["<node_internals>/**"],
    });
    assert.ok(ok, "startDebugging(pwa-node attach) returned true");

    try {
      // Give the session time to attach, load calc.ts, resolve the .bynk
      // breakpoint through the map, continue past entry, and pause when the
      // interval calls dbl().
      const deadline = Date.now() + 25_000;
      while (Date.now() < deadline && !hitBreakpoint) {
        await new Promise((r) => setTimeout(r, 200));
      }
      assert.ok(
        hitBreakpoint,
        "a breakpoint set in calc.bynk bound (reverse-mapped through calc.ts.map) and paused execution on a real run",
      );
    } finally {
      tracker.dispose();
      vscode.debug.removeBreakpoints([bp]);
      await vscode.debug.stopDebugging();
    }
  });
});
