# bynk-wasm

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

The **[Bynk](https://github.com/accuser/bynk) compiler compiled to a wasm
module**, backing the in-browser REPL and [`playground/`](https://github.com/accuser/bynk/tree/main/playground).

> **Not a published crate.** `bynk-wasm` is `publish = false` — an internal build
> artefact, not on crates.io or docs.rs. It is produced by the `playground/`
> wasm build, never via `cargo install`.

## What it does

A single entry point — `bynk_compile` (wasm) / `compile` (native) — takes
in-memory Bynk source and returns a runnable **JavaScript module graph** plus
diagnostics, with **no filesystem and no `tsc`**:

```text
source ─▶ bynk_emit::compile_in_memory  ─▶ ProjectOutput (TypeScript)
       ─▶ bynk_strip::strip_project_to_js ─▶ ProjectOutput (JavaScript)
       ─▶ { files: [{ path, contents }], diagnostics }
```

The returned graph is the complete set the browser links: the user module,
`runtime.js`, the `bynk-browser.js` binding, and `compose.js`. Three companion
entries back the playground's editor: `bynk_analyze` returns diagnostics only,
for live on-type checking; `bynk_hover` returns the inferred type at an offset;
and `bynk_complete` returns completion candidates at an offset. Every entry
compiles for the `browser` platform.

The pipeline reuses the on-disk compile path wholesale (first-party injection,
the per-platform binding, the strip-only emitter), so the in-browser result
matches the CLI's.

## Build

The crate builds two ways:

- **`cdylib`** — the `wasm32` output consumed by the playground's build.
- **`rlib`** — lets the native test build link the same logic, so the compile
  path is verified without a browser.

It is built as part of the `playground/` toolchain rather than installed on its
own.

## License

Licensed under either of [Apache-2.0](https://github.com/accuser/bynk/blob/main/LICENSE-APACHE) or
[MIT](https://github.com/accuser/bynk/blob/main/LICENSE-MIT) at your option.
