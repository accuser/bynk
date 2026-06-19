// Fails if the esbuild bundle (out/extension.js) has any unresolved external
// `require` other than `vscode` (which the host provides). This guards against
// the packaging regression shipped in 0.20.0, where the VSIX excluded
// node_modules but the code wasn't bundled, so `vscode-languageserver-protocol`
// (a transitive dep of vscode-languageclient) was missing at activation.
import { readFileSync } from "node:fs";
import { builtinModules } from "node:module";

const src = readFileSync(new URL("../out/extension.js", import.meta.url), "utf8");
const requires = [
  ...new Set(
    [...src.matchAll(/require\(['"]([^'"]+)['"]\)/g)].map((m) => m[1]),
  ),
];

const isBuiltin = (name) => builtinModules.includes(name.replace(/^node:/, ""));
const unresolved = requires.filter(
  (name) => !name.startsWith(".") && !isBuiltin(name) && name !== "vscode",
);

if (unresolved.length > 0) {
  console.error(
    "Unbundled external require(s) in out/extension.js:",
    unresolved,
  );
  console.error(
    "The extension must bundle all deps except `vscode`. Check the esbuild build.",
  );
  process.exit(1);
}

console.log("Bundle OK — only Node builtins + `vscode` are external.");
