# Install

Karn is pre-1.0 and is not yet published to a package registry. You install it
by **building from source** with a recent Rust toolchain.

## Prerequisites

- **Rust** (stable, 2024 edition). Install via [rustup](https://rustup.rs/).
- **Git**, to clone the repository.
- A **Node.js / TypeScript** toolchain if you want to type-check or run the
  emitted TypeScript (and `wrangler` if you want to deploy to Cloudflare
  Workers).

## Build and install the compiler

Clone the repository and install the `karnc` binary with `cargo`:

```sh
git clone https://github.com/accuser/karn.git
cd karn
cargo install --path karnc
```

This puts `karnc` on your `PATH` (under `~/.cargo/bin` by default). Verify it:

```sh
karnc --help
```

`karnc` exposes four commands:

| Command          | Purpose                                            |
|------------------|----------------------------------------------------|
| `karnc compile`  | Compile Karn source to TypeScript.                 |
| `karnc check`    | Type-check without emitting.                        |
| `karnc fmt`      | Format Karn source.                                |
| `karnc test`     | Compile and run `test` blocks.                     |

See the [CLI reference](../reference/cli.md) for every flag and exit code.

## Optional: the language server

For editor integration (diagnostics, hover, go-to-definition), install the
language server:

```sh
cargo install --path karn-lsp
```

This provides the `karnc-lsp` binary. Most users consume it through the VS Code
extension rather than invoking it directly — see
[Set up editor support](../how-to/tooling/editor-support.md).

## Next steps

- [Compile your first program](../tutorials/01-first-program.md)
