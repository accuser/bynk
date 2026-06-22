// Slice 5 spike (workerd half) — and now a regression guard for its verdict.
//
// The Node spike (debug_values.test.ts) proved `customDescriptionGenerator`
// renders Bynk values. This file spiked whether it reaches the `workerd`
// debuggee over `wrangler`'s inspector. **It does not:** with the generator set,
// js-debug throws "Error processing variables: unreachable" — `workerd` rejects
// the in-debuggee function evaluation the generator requires (its runtime
// restricts dynamic code). *Raw* variable reading works fine. So semantic values
// are Node-only in v1; the dev (workerd) path must **not** inject the generator
// (it would break variable inspection outright) — workerd parity is the deferred
// custom-adapter follow-on. This test guards that: variable inspection works on a
// real worker, with the generator off, exactly as the provider's dev mode runs.
//
// Skipped without `wrangler`/`bynkc`/`node` (CI has no Cloudflare runtime); built
// inside the workspace folder like slice 4's workerd test.

import * as assert from "assert";
import * as path from "path";
import * as fs from "fs";
import { spawn, spawnSync, ChildProcess } from "child_process";
import * as vscode from "vscode";

function have(tool: string): boolean {
  try {
    return spawnSync(tool, ["--version"], { stdio: "ignore" }).status === 0;
  } catch {
    return false;
  }
}

describe("Bynk debug values (workerd)", () => {
  let dir: string;
  let wrangler: ChildProcess | undefined;

  after(() => {
    wrangler?.kill();
    try {
      spawnSync("pkill", ["-f", "workerd"], { stdio: "ignore" });
    } catch {
      /* best-effort */
    }
    if (dir) {
      try {
        fs.rmSync(dir, { recursive: true, force: true });
      } catch {
        /* best-effort */
      }
    }
  });

  it("reads a worker handler's tagged local (raw — the generator is Node-only)", async function () {
    if (!have("wrangler") || !have("bynkc") || !have("node")) this.skip();
    this.timeout(90_000);
    const verbose = process.env.BYNK_DEBUG_SPIKE === "verbose";

    const workspace = path.resolve(__dirname, "../../../test/fixtures/workspace");
    dir = fs.realpathSync(fs.mkdtempSync(path.join(workspace, ".vwdbg-")));
    fs.mkdirSync(path.join(dir, "src"));
    fs.writeFileSync(path.join(dir, "bynk.toml"), "[project]\nname = \"svc\"\n");
    // `opt` is a tagged Option local, in scope at the `Logger.info` line.
    const svc = [
      "context svc",
      "",
      "consumes bynk { Logger }",
      "",
      "service api from http {",
      "\ton GET(\"/\") by v: Visitor () -> Effect[HttpResult[String]] given Logger {",
      "\t\tlet opt = Some(\"hi\")",
      "\t\tlet _ <- Logger.info(\"hit\")",
      "\t\tmatch opt {",
      "\t\t\tSome(s) => Ok(s)",
      "\t\t\tNone => Ok(\"none\")",
      "\t\t}",
      "\t}",
      "}",
      "",
    ].join("\n");
    fs.writeFileSync(path.join(dir, "src", "svc.bynk"), svc);
    const bpLine = svc.split("\n").findIndex((l) => l.includes("Logger.info")) + 1;

    const build = path.join(dir, "build");
    const compile = spawnSync(
      "bynkc",
      ["compile", path.join(dir, "src"), "--output", build, "--target", "workers"],
      { encoding: "utf8" },
    );
    assert.strictEqual(compile.status, 0, `bynkc compile failed: ${compile.stderr}`);
    const workerDir = path.join(build, "workers", "svc");

    const base = 9500 + (process.pid % 200) * 2;
    const inspectorPort = base + 1;
    const appPort = base;
    wrangler = spawn(
      "wrangler",
      ["dev", "--inspector-port", String(inspectorPort), "--port", String(appPort)],
      { cwd: workerDir, stdio: ["ignore", "ignore", "pipe"] },
    );

    const ready = await waitFor(async () => {
      try {
        const r = await fetch(`http://127.0.0.1:${inspectorPort}/json`);
        return ((await r.json()) as any[]).length > 0;
      } catch {
        return false;
      }
    }, 45_000);
    assert.ok(ready, "wrangler dev inspector never became reachable");

    let optValue: string | undefined;
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
            if (!frame || !String(frame.source?.name ?? "").endsWith("svc.bynk")) {
              void s.customRequest("continue", { threadId });
              return;
            }
            try {
              const sc: any = await s.customRequest("scopes", { frameId: frame.id });
              for (const scope of sc.scopes) {
                const vars: any = await s.customRequest("variables", {
                  variablesReference: scope.variablesReference,
                });
                const opt = vars.variables.find((x: any) => x.name === "opt");
                if (opt) {
                  optValue = opt.value;
                  if (verbose) console.log("[var] opt =>", opt.value);
                }
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
        vscode.Uri.file(path.join(dir, "src", "svc.bynk")),
        new vscode.Position(bpLine - 1, 0),
      ),
    );
    vscode.debug.addBreakpoints([bp]);

    // No `customDescriptionGenerator`: the dev (workerd) path omits it — injecting
    // it makes wrangler's inspector throw "Error processing variables: unreachable"
    // and breaks all variable reading. This mirrors how the provider runs dev mode.
    const ok = await vscode.debug.startDebugging(undefined, {
      type: "node",
      request: "attach",
      name: "bynk-values-workerd-spike",
      port: inspectorPort,
      resolveSourceMapLocations: null,
      skipFiles: ["<node_internals>/**"],
    });
    assert.ok(ok, "startDebugging returned true");

    try {
      const deadline = Date.now() + 30_000;
      while (Date.now() < deadline && optValue === undefined) {
        fetch(`http://127.0.0.1:${appPort}/`).catch(() => {});
        await new Promise((r) => setTimeout(r, 500));
      }
      if (verbose) console.log("[result] optValue=", optValue);
      // Variable inspection works on the worker; the value reads as the raw tagged
      // shape (semantic rendering is Node-only — see this file's header).
      assert.ok(
        optValue !== undefined && optValue.includes("Some"),
        `opt is readable on workerd, got ${optValue}`,
      );
    } finally {
      tracker.dispose();
      vscode.debug.removeBreakpoints([bp]);
      await vscode.debug.stopDebugging();
    }
  });
});

async function waitFor(cond: () => Promise<boolean>, timeoutMs: number): Promise<boolean> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await cond()) return true;
    await new Promise((r) => setTimeout(r, 500));
  }
  return false;
}
