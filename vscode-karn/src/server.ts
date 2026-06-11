// Server provisioning for the Karn LSP.
//
// Resolution order, most explicit first:
//   1. `karn.executablePath` setting (absolute path the user pinned)
//   2. `karnc-lsp` on PATH (a dev/global install)
//   3. a previously-downloaded binary cached in the extension's global storage
//   4. download-on-activate: fetch the matching per-platform `karnc-lsp` from
//      this version's GitHub Release, verify it against the release's
//      SHA256SUMS, cache it, and use it.
//
// The download fetches the *raw* binary asset (`karnc-lsp-<target>[.exe]`), not
// an archive, so the extension needs no tar/zip handling — just fetch + verify
// + chmod.

import * as path from "node:path";
import * as fs from "node:fs";
import * as cp from "node:child_process";
import * as crypto from "node:crypto";

import * as vscode from "vscode";

export type ResolvedServer = {
  path: string;
  source: "setting" | "path" | "cached" | "downloaded";
};

/** The compiler/server version this extension build targets, e.g. `v0.23.0`.
 *  Declared in package.json as `karnServerVersion` so it is visible to users
 *  and bumped alongside a server release. */
export function serverVersion(context: vscode.ExtensionContext): string {
  const v = context.extension.packageJSON.karnServerVersion as
    | string
    | undefined;
  if (!v) {
    throw new Error(
      "vscode-karn is misconfigured: package.json is missing `karnServerVersion`.",
    );
  }
  return v;
}

/** `<owner>/<repo>` parsed from package.json `repository.url`. */
function repoSlug(context: vscode.ExtensionContext): string {
  const url: string = context.extension.packageJSON.repository?.url ?? "";
  const m = url.match(/github\.com[/:]([^/]+\/[^/.]+)/);
  return m ? m[1] : "accuser/karn";
}

/** Map the host to the Rust target triple used by the release assets, or
 *  undefined when no prebuilt server is published for this platform. */
export function targetTriple(
  platform: NodeJS.Platform = process.platform,
  arch: string = process.arch,
): string | undefined {
  switch (`${platform}-${arch}`) {
    case "win32-x64":
      return "x86_64-pc-windows-msvc";
    case "darwin-arm64":
      return "aarch64-apple-darwin";
    case "darwin-x64":
      return "x86_64-apple-darwin";
    case "linux-x64":
      return "x86_64-unknown-linux-gnu";
    case "linux-arm64":
      return "aarch64-unknown-linux-gnu";
    default:
      return undefined;
  }
}

/** The release asset basename for the raw server binary on `target`. */
export function serverAssetName(target: string): string {
  const exe = target.includes("windows") ? ".exe" : "";
  return `karnc-lsp-${target}${exe}`;
}

function cachedServerPath(
  context: vscode.ExtensionContext,
  version: string,
  target: string,
): string {
  const exe = target.includes("windows") ? ".exe" : "";
  return path.join(
    context.globalStorageUri.fsPath,
    "server",
    version,
    `karnc-lsp${exe}`,
  );
}

function findOnPath(bin: string): string | undefined {
  const PATH = process.env.PATH ?? "";
  const sep = process.platform === "win32" ? ";" : ":";
  for (const dir of PATH.split(sep)) {
    if (!dir) continue;
    const candidate = path.join(dir, bin);
    if (fs.existsSync(candidate)) return candidate;
    if (process.platform === "win32" && fs.existsSync(candidate + ".exe")) {
      return candidate + ".exe";
    }
  }
  return undefined;
}

/** Resolve a server without downloading. Returns undefined if only a download
 *  would satisfy it (the caller decides whether to fetch). */
export function resolveExistingServer(
  context: vscode.ExtensionContext,
): ResolvedServer | undefined {
  const configured = vscode.workspace
    .getConfiguration("karn")
    .get<string>("executablePath", "")
    .trim();
  if (configured) {
    // An explicit setting is honoured as-is: absolute path, or a name on PATH.
    if (path.isAbsolute(configured) && fs.existsSync(configured)) {
      return { path: configured, source: "setting" };
    }
    const onPath = findOnPath(configured);
    if (onPath) return { path: onPath, source: "setting" };
    // Configured but missing — surface that rather than silently downloading.
    return undefined;
  }

  const onPath = findOnPath("karnc-lsp");
  if (onPath) return { path: onPath, source: "path" };

  const target = targetTriple();
  if (target) {
    const cached = cachedServerPath(context, serverVersion(context), target);
    if (fs.existsSync(cached)) return { path: cached, source: "cached" };
  }
  return undefined;
}

/** Parse a SHA256SUMS file (`<hex>  <basename>` per line) for `basename`. */
export function expectedSha(sumsText: string, basename: string): string | undefined {
  for (const line of sumsText.split(/\r?\n/)) {
    const m = line.trim().match(/^([0-9a-f]{64})\s+\*?(.+)$/i);
    if (m && path.basename(m[2]) === basename) return m[1].toLowerCase();
  }
  return undefined;
}

async function fetchBuffer(url: string): Promise<Buffer> {
  const res = await fetch(url, { redirect: "follow" });
  if (!res.ok) {
    throw new Error(`GET ${url} → ${res.status} ${res.statusText}`);
  }
  return Buffer.from(await res.arrayBuffer());
}

/** Download, verify, cache, and return the path to the server binary for this
 *  platform and the extension's pinned server version. Throws on any failure
 *  (unsupported platform, network, checksum mismatch). */
export async function downloadServer(
  context: vscode.ExtensionContext,
  output: vscode.OutputChannel,
): Promise<string> {
  const target = targetTriple();
  if (!target) {
    throw new Error(
      `No prebuilt karnc-lsp for ${process.platform}/${process.arch}. ` +
        "Build it (`cargo build --release -p karn-lsp`) and set `karn.executablePath`.",
    );
  }
  const version = serverVersion(context);
  const slug = repoSlug(context);
  const asset = serverAssetName(target);
  const base = `https://github.com/${slug}/releases/download/${version}`;

  return vscode.window.withProgress(
    {
      location: vscode.ProgressLocation.Notification,
      title: `Karn: downloading language server ${version} (${target})`,
      cancellable: false,
    },
    async () => {
      output.appendLine(`[server] fetching ${base}/SHA256SUMS`);
      const sums = (await fetchBuffer(`${base}/SHA256SUMS`)).toString("utf8");
      const want = expectedSha(sums, asset);
      if (!want) {
        throw new Error(`SHA256SUMS has no entry for ${asset}`);
      }

      output.appendLine(`[server] fetching ${base}/${asset}`);
      const bin = await fetchBuffer(`${base}/${asset}`);
      const got = crypto.createHash("sha256").update(bin).digest("hex");
      if (got !== want) {
        throw new Error(
          `checksum mismatch for ${asset}: expected ${want}, got ${got}`,
        );
      }

      const dest = cachedServerPath(context, version, target);
      await fs.promises.mkdir(path.dirname(dest), { recursive: true });
      // Write-then-rename so a crash mid-write can't poison the cache with a
      // truncated binary (resolveExistingServer trusts an existing file).
      const tmp = `${dest}.tmp-${process.pid}`;
      await fs.promises.writeFile(tmp, bin, { mode: 0o755 });
      await fs.promises.rename(tmp, dest);
      output.appendLine(`[server] cached at ${dest}`);
      return dest;
    },
  );
}

/** The server's `--version` string, or undefined if it can't be run. */
export function readServerVersion(serverPath: string): string | undefined {
  try {
    return cp
      .spawnSync(serverPath, ["--version"], { timeout: 2000 })
      .stdout?.toString("utf8")
      .trim();
  } catch {
    return undefined;
  }
}
