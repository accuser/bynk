# 0238 — The VS Code extension resolves its compiler through `bynk`, not by reimplementing `bynkc` lookup

- **Status:** Accepted (v0.212.1)

**Context.** The extension shelled `bynkc` directly for three surfaces — the
`bynkc: check` build task, the Test Explorer (`testing.ts`), and test debugging
(`debug.ts`'s `startTest`) — resolving it as `bynk.compilerPath` setting, else a
bare `bynkc` on `PATH`. The `bynk` driver resolves the same binary more richly:
`$BYNK_BYNKC` override, else `PATH`, else a `bynkc` sibling of the running `bynk`
(#486). A developer who reaches `bynk` first (the documented front-end) and has
`bynkc` reachable only via `BYNK_BYNKC` or as a `bynk`-sibling has a working CLI
but a broken Test Explorer / check task, with only a bare "not found" to go on.

`bynk check`/`bynk fmt`/`bynk test` (#487, v0.138) already exist and mirror
`bynkc`'s flag surfaces exactly, `test` explicitly delegating through the
driver's own resolution for this reason.

**Decision.** The three extension surfaces shell `bynk` instead of `bynkc`:
`bynk check . --format short` (the build task), `bynk test . --format json`
(discovery/run), and `bynk test --inspect` (debug). This is a straight swap —
`bynk check`/`fmt` run the pipeline in-process and `bynk test` delegates to the
driver-resolved `bynkc`, forwarding flags and inheriting stdio verbatim, so the
JSON/diagnostic shapes and the debug inspector handshake are unchanged.

The `bynk.compilerPath` setting is kept (not renamed, for config compatibility)
but now means "pin an exact `bynkc`, passed through as `BYNK_BYNKC` to `bynk`"
rather than "the binary to shell directly" — the driver already honours that
override the same way `bynk dev`'s escape hatch does. The existing
`bynk.bynkPath` setting (previously scoped to the `bynk dev` debug session) is
reused as the resolution point for all three surfaces rather than adding a
second driver-path setting.

**Consequences.** A driver-first install now works uniformly across the CLI and
the editor. The extension no longer needs its own compiler-lookup logic to stay
in sync with the driver's; `#484` (LSP/compiler version skew) is unaffected —
`bynkc-lsp` resolution is a separate, unrelated mechanism. The CI job that runs
the extension's integration tests now also builds `-p bynk` (previously only
`bynk-lsp` + `bynkc`), since the debug-provider and test-CodeLens tests exercise
this path for real.
