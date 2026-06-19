# Install

Bynk is pre-1.0 and is not yet published to a package registry. You install it
by **building from source** with a recent Rust toolchain.

## Prerequisites

- **Rust** (stable, 2024 edition). Install via [rustup](https://rustup.rs/).
- **Git**, to clone the repository.
- A **Node.js / TypeScript** toolchain if you want to type-check or run the
  emitted TypeScript (and `wrangler` if you want to deploy to Cloudflare
  Workers).

## Build and install the compiler

Clone the repository and install the `bynkc` binary with `cargo`:

```sh
git clone https://github.com/accuser/bynk.git
cd bynk
cargo install --path bynkc
```

This puts `bynkc` on your `PATH` (under `~/.cargo/bin` by default). Verify it:

```sh
bynkc --help
```

## Check your environment with `bynk doctor`

Rather than hunting down `node`, `tsc`, and `wrangler` one broken command at a
time, install the **`bynk` driver** and let it tell you exactly what your
machine is ready for:

```sh
cargo install --path bynk
bynk doctor
```

`bynk doctor` groups its checks by capability — compile/check/fmt, `bynk test`,
`dev`/deploy, editor support — and prints the exact remedy for anything missing.
It is the recommended first step: the prerequisites below are *checked*, not just
listed. See [Check your environment with `bynk
doctor`](../guides/editor-and-tooling/doctor.md) for the capability groups, exit
codes, and `--format` outputs.

`bynkc` exposes four commands:

| Command          | Purpose                                            |
|------------------|----------------------------------------------------|
| `bynkc compile`  | Compile Bynk source to TypeScript.                 |
| `bynkc check`    | Type-check without emitting.                        |
| `bynkc fmt`      | Format Bynk source.                                |
| `bynkc test`     | Compile and run `test` blocks.                     |

See the [CLI reference](../reference/cli.md) for every flag and exit code.

## Optional: the language server

For editor integration (diagnostics, hover, go-to-definition), install the
language server:

```sh
cargo install --path bynk-lsp
```

This provides the `bynkc-lsp` binary. Most users consume it through the VS Code
extension rather than invoking it directly — see
[Set up editor support](../guides/editor-and-tooling/editor-support.md).

## Next steps

- [Compile your first program](../tutorials/01-first-program.md)
