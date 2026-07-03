# bynk-strip

[![crates.io](https://img.shields.io/crates/v/bynk-strip.svg)](https://crates.io/crates/bynk-strip)
[![docs.rs](https://img.shields.io/docsrs/bynk-strip)](https://docs.rs/bynk-strip)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

**Strip-only TypeScript → JavaScript for [Bynk](https://github.com/accuser/bynk)'s
first-class JS artefact.** It erases TypeScript type syntax while preserving
every runtime construct and value import, so a JS artefact is simply the
emitter's TypeScript with its types deleted.

Because the Bynk emitter is **strip-only** — every emitted `.ts` is erasable by
pure type-stripping, never a type-directed lowering (no parameter properties,
`enum`s, or `namespace`s) — the transform here is total and lossless for runtime
behaviour: it only deletes type syntax, it never has to rewrite semantics.

- `strip_types` — strip one TypeScript source string to JavaScript.
- `strip_project_to_js` — rewrite a compiled `bynk-emit` project to a JS
  artefact: each `.ts` module is stripped and renamed to `.js`, the
  `tsconfig.json` is dropped, and every other file passes through unchanged.
- `StripError` — a strip failure; for valid emitter output it should never
  occur, so it signals an emitter or toolchain bug rather than user error.

The engine is [`oxc`](https://crates.io/crates/oxc) — a pure-Rust TypeScript
parser, type-erasing transform, and codegen — so neither `bynkc --emit js` nor
the in-browser compile path pulls in Node or `tsc`, and the crate compiles to
`wasm32` for the playground. Stripping is configured for pure type-erasure
(matching Node's `stripTypeScriptTypes`): every *value* import is kept even when
unused, and only `import type` / `type` specifiers are elided.

## Where it sits

```text
bynk-syntax  ◀── bynk-render · bynk-fmt · bynk-check ◀── bynk-emit ◀── bynk-ide
                                                             ◀── bynk-strip
```

`bynk-strip` sits above `bynk-emit`, turning its `ProjectOutput` into a JS
artefact. The dependency runs one way only — `bynk-emit` does not depend on
`bynk-strip` — so the language server (via `bynk-ide` → `bynk-emit`) never pulls
in `oxc`. The `bynkc`, `bynk`, and `bynk-lsp` binaries are front-ends over the
compiler set.

## Use

```toml
[dependencies]
bynk-strip = "0.125"
```

```rust
let js = bynk_strip::strip_types("const n: number = 1;", "main.ts")?;
assert_eq!(js.trim(), "const n = 1;");
```

See the [API docs](https://docs.rs/bynk-strip) for the full surface.

## License

Licensed under either of [Apache-2.0](https://github.com/accuser/bynk/blob/main/LICENSE-APACHE) or
[MIT](https://github.com/accuser/bynk/blob/main/LICENSE-MIT) at your option.
