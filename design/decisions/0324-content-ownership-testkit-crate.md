# 0324 — Cross-crate test fixtures get a new `bynk-testkit` crate, built on production discovery

- **Status:** Accepted (v0.247.13)

**Context.** `bynk-emit/src/lib.rs` already gates a `#[cfg(test)]
pub(crate) mod testkit` (`bynk-emit/src/testkit.rs`) with two helpers used
only by `bynk-emit`'s own tests. The content-ownership track (#1086) needs an
equivalent for ~125 call sites across `bynk-ide`'s inline tests,
`bynk-lsp/tests`, `bynkc/tests`/`bynk/tests`, and one of `bynk-emit`'s own
`#[cfg(test)]` sites, that today rely on
`bynk-emit/src/project/discovery.rs`'s disk fallback via
`diagnose_project(&root, &HashMap::new())` or `CompileOptions::single`/`::split`
with no `.sources(...)` chained. Extending `bynk-emit`'s testkit was
considered and rejected: it's `pub(crate)` by design, and `bynk-lsp/tests`
could not reach it without `bynk-lsp` taking a production-excluded
dependency on `bynk-emit` just for tests.

**Decision.** A new dev-only workspace crate, `bynk-testkit`, depending on
`bynk-ide` (and, where needed, `bynk-emit` — unproblematic for a dev-only
crate). Its core helper is built directly on `bynk-ide`'s existing `pub fn
discover_files(roots: &AnalysisRoots) -> Vec<PathBuf>`
(`bynk-ide/src/lib.rs:234`), the same resolution production analysis already
uses — not a second, independent directory walk. `bynk-testkit` becomes a
`[dev-dependencies]` entry of `bynk-ide`, `bynk-lsp`, and `bynkc` (which
already dev-depends on `bynk-ide` for its own integration tests,
`bynkc/Cargo.toml`). Exact helper signatures for the two call-site shapes (a
`HashMap`-returning helper for `diagnose_project*`, a `CompileOptions`-returning
helper for `::single`/`::split`) are proven on a representative handful of
call sites (`design/tracks/content-ownership.md` §4 slice 4) before the full
migration (slice 5), not fixed by this ADR.

**Consequences.** The drift risk `content-ownership.md` names — a test
helper resolving include/exclude differently from `discover_bynk_files` and
silently missing files — is closed structurally: there is only one walk
implementation (`bynk_ide::discover_files`) for both production and test
code to call. `bynk-testkit` ships no production code and is invisible to
`fs_below_driver`'s probe. Once slice 5 completes, `discovery.rs`'s content
fallback (slice 6) has no caller left needing it.
