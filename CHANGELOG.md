# Changelog

## Unreleased — project renamed from **Karn** to **Bynk**

The project, its toolchain, and its in-language surface were renamed from
**Karn** to **Bynk**. This is a **breaking** change for existing sources.

### Toolchain

- The compiler binary `karnc` is now **`bynkc`**; the driver `karn` is now
  **`bynk`** (e.g. `bynk build`, `bynk test`, `bynk doctor`). The language
  server `karnc-lsp` is now **`bynkc-lsp`**.
- The project manifest `karn.toml` is now **`bynk.toml`**.
- The source file extension `.karn` is now **`.bynk`**.

### In-language reserved surface (breaking)

The reserved namespace `karn` is renamed to **`bynk`**. Update your sources:

- `consumes karn { … }` → `consumes bynk { … }`
- platform adapters `karn.cloudflare` / `karn.node` / … → `bynk.cloudflare` /
  `bynk.node` / …
- stdlib units `karn.list` / `karn.map` / `karn.string` → `bynk.list` /
  `bynk.map` / `bynk.string`

### Diagnostics & runtime

- Every diagnostic code is reprefixed `karn.*` → **`bynk.*`** (e.g.
  `karn.namespace.reserved` → `bynk.namespace.reserved`).
- The internal HTTP dispatch prefix `/_karn/` is now **`/_bynk/`** and the
  cross-context caller header `X-Karn-Caller` is now **`X-Bynk-Caller`**.

### Migrating a project

1. Rename `karn.toml` → `bynk.toml` and every `*.karn` source → `*.bynk`.
2. Replace `consumes karn` with `consumes bynk` and every `karn.<platform>` /
   `karn.<stdlib>` reference with its `bynk.<…>` equivalent.
3. Recompile with `bynkc` (or `bynk build`).
