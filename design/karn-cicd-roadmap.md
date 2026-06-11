# Karn CI/CD — roadmap

A forward plan for the build, test, and release pipeline. **Tiers 1–3 are implemented**
(see status below); Tier 4 remains. A design reference, not a per-increment proposal.

---

## 1. Current state (after the Tier 1–3 pass)

**CI** (`.github/workflows/ci.yml`, on push / PR / weekly Mon 07:00 UTC) — ten jobs:

- `fmt`, `clippy` — unchanged.
- `test` — `cargo test --workspace --locked` with `KARN_REQUIRE_TSC=1`, now **matrixed across
  ubuntu / macOS / windows** (`fail-fast: false`); `typescript@5` pinned.
- `msrv` — `cargo check --workspace --locked` on the declared `rust-version` (1.85).
- `docs` — mdBook via `taiki-e/install-action` (prebuilt) + linkcheck.
- `extension` — `npm ci` → `tsc --noEmit` → esbuild bundle → `scripts/check-bundle.mjs`
  (no unbundled external `require`) → `vsce package`.
- `grammar` — `tree-sitter generate` + `tree-sitter test` (the corpus).
- `audit` — `cargo audit` (RustSec).
- `deny` — `cargo-deny` (licences / bans / sources, per `deny.toml`).
- `dependency-review` — `actions/dependency-review-action` on PRs.

**Release** (`release.yml`): two-phase, five-target binaries + GitHub Release; manual-dispatch
publish to crates.io + npm. Now also: **`SHA256SUMS`** over the archives, **signed build
provenance** (`actions/attest-build-provenance`), **npm `--provenance`**, and **crates.io OIDC
Trusted Publishing** (`rust-lang/crates-io-auth-action`, replacing the long-lived token).
`typescript@5` pinned; tests `--locked`.

**Pages** (`pages.yml`): mdBook via `taiki-e/install-action`; deploys to GitHub Pages on `main`.

**Config**: `.gitattributes` (LF normalisation — makes the Windows test leg's byte-exact
comparison stable); `deny.toml`; `rust-version = "1.85"` in the workspace manifest.

**Dependabot**: cargo, github-actions, npm (`tree-sitter-karn`, `vscode-karn`).

---

## 2. Done — Tiers 1–3

### Tier 1 — proven holes ✅

- **CI the VS Code extension** — the `extension` job; the smoke step (`check-bundle.mjs`) is
  exactly the gate that would have caught the 0.20.0 transitive-dep crash. *(The esbuild
  bundling fix had regressed out of the working tree — `package.json` back to `tsc`, version
  0.20.0, while `.vscodeignore` still excluded `node_modules` — and was restored as part of
  this.)*
- **tree-sitter grammar corpus** — the `grammar` job (`generate` + `test`).
- **Multi-OS test matrix** — ubuntu / macOS / windows, with `.gitattributes` LF normalisation
  so byte-exact fixtures don't fail spuriously on Windows.

### Tier 2 — release integrity & supply chain ✅

- **Checksums + provenance** — `SHA256SUMS` + `attest-build-provenance` on the archives;
  npm `--provenance`.
- **crates.io OIDC Trusted Publishing** — replaces `CARGO_REGISTRY_TOKEN`.
- **cargo-deny** — `deny.toml` + the `deny` job (advisories overlap `audit` by design;
  unique value is licences/bans/sources).
- **PR dependency-review** — the `dependency-review` job.

### Tier 3 — reproducibility hygiene ✅

- **Pin `typescript@5`** (CI `test` + release `verify`).
- **`--locked`** on CI `test` (release already had it).
- **Declared MSRV** (`rust-version = "1.85"`) — *and* the `msrv` CI leg that actually builds
  on it, so the claim is verified, not asserted.
- **Prebuilt mdBook** via `taiki-e/install-action` (CI `docs` + `pages`).

### First-run caveats (validate on the first GitHub run)

- **Windows / macOS test legs** may surface genuine byte-exactness or path bugs — the point
  of adding them; the LF guard removes the spurious CRLF failures. *(The predictable harness
  failure is already fixed pre-flight: the five tsc-driven test files probe via `where` on
  Windows and spawn npm's `.cmd` shims through `cmd /C` — Rust's CreateProcess refuses batch
  scripts directly — so the first Windows run measures the product, not the tooling.)*
- **Private-repo gates** (self-healing at the v1.0.0 public flip): `dependency-review` is
  conditioned on repository visibility (the API needs GHAS on private repos), and the
  `ubuntu-24.04-arm` release leg is commented out (free arm runners are public-only; on a
  private repo it queues forever and `needs: binaries` would block the whole release).
- **MSRV 1.85** is the edition-2024 floor; if the code needs newer, the `msrv` leg goes red —
  raise `rust-version`, don't lower the leg.
- **`deny.toml` licence allow-list** is broad but may miss one transitive licence on first
  run — add it (or an `exceptions` entry).
- **crates.io OIDC** needs a one-time Trusted Publisher configured per crate (karn-grammar,
  karnc, karn-fmt, karn-lsp) before the next publish; keep `CARGO_REGISTRY_TOKEN` until then.
- **Renaming a CI job requires a matching ruleset update** — the `main protection` ruleset
  pins required checks *by job name*, and a required check that no longer exists waits as
  "Expected" forever (hit on this PR: the old "Test suite (workspace, …)" name vs the new
  matrix legs). The required set is now the eleven always-on jobs; `dependency-review` is
  deliberately unrequired (it skips while the repo is private).

---

## 3. Remaining — Tier 4 (distribution polish; higher effort)

- **Extension + grammar release automation.** Build per-platform VSIXs that **bundle
  `karnc-lsp`** (tying `release.yml`'s binaries to the extension) and publish to the VS Code
  Marketplace + Open VSX — the tooling roadmap's B-0/B-2 expressed in CI. *(Needs marketplace
  tokens.)*
- **Binary signing / notarisation.** macOS notarisation + Windows signing for the downloaded
  binaries (Gatekeeper / SmartScreen friction) — **needs certificates.**
- **Supply-chain posture.** Optionally OpenSSF Scorecard + SHA-pinning the actions (tag-pinned
  today; Dependabot covers github-actions).

See [`karn-tooling-roadmap.md`](karn-tooling-roadmap.md) — Tier 4's extension publishing and
the server-provisioning work there (B-0) are the same effort from two angles.
