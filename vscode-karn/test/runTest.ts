import * as path from "path";

import { runTests } from "@vscode/test-electron";

// Bootstraps a real VS Code instance with the extension under
// `--extensionDevelopmentPath` plus the fixture workspace, then runs the Mocha
// suite inside the extension host. See test/suite/index.ts for the runner.
async function main() {
  try {
    // The extension root (manifest + bundled `out/extension.js`).
    const extensionDevelopmentPath = path.resolve(__dirname, "../../");
    // The compiled Mocha entry point (out/test/suite/index.js).
    const extensionTestsPath = path.resolve(__dirname, "./suite/index");
    // The fixture project the host opens (a `karn.toml` + `src/`).
    const workspace = path.resolve(
      __dirname,
      "../../test/fixtures/workspace",
    );

    // The extension resolves `karnc-lsp` from the `karn.executablePath`
    // setting, then PATH, then a cached download. Prepend the repo's release
    // dir so activation connects to a locally-built server deterministically —
    // tests must never hit the download-on-activate path.
    const serverDir = path.resolve(__dirname, "../../../target/release");
    process.env.PATH = `${serverDir}${path.delimiter}${process.env.PATH ?? ""}`;

    await runTests({
      extensionDevelopmentPath,
      extensionTestsPath,
      // Open the fixture workspace; `--disable-extensions` keeps third-party
      // extensions out of the host (the extension under test still loads).
      launchArgs: [workspace, "--disable-extensions"],
    });
  } catch (err) {
    console.error("Failed to run integration tests:", err);
    process.exit(1);
  }
}

main();
