---
level: patch
changelog: "**Breaking (Rust API, not language surface):** `bynkc` no longer re-exports fourteen whole modules from `bynk-check`/`bynk-emit` (`bynkc::checker`, `bynkc::resolver`, `bynkc::emitter`, `bynkc::project`, and ten others) — only its ~30 item re-exports (`CompileOptions`, `compile_project`, `BuildTarget`, …) remain public"
closes_rule: R10.4
---

## ADR: narrow-bynkc-public-api
title: bynkc's published Rust API is its item re-exports, not its module structure
summary: The fourteen whole-module re-exports are deleted; bynkc keeps its lib, its ~30 item re-exports, and cargo install bynkc

**Context.** `bynkc` carries both a `[lib]` and a `[[bin]]`. The library is a
thin re-export facade, and nothing in the workspace depends on it — the
`bynk` driver states outright that it no longer depends on the `bynkc` crate,
and `bynk-ide` was demoted to a dev-dependency for the same reason. But
`bynkc` **is published** to crates.io, so its
`pub use bynk_check::{actors, builtin_names, checker, expr_types, firstparty,
hints, index, kernel_methods, locals, requirements, resolver, store_ops}` and
`pub use bynk_emit::{emitter, project}` put `bynkc::checker::Ty` and
`bynkc::resolver::*` on crates.io as public API of a released crate — a
crate whose internal decomposition into leaf crates (`bynk-syntax` →
`bynk-check` → `bynk-emit`) was itself a deliberate architectural choice, now
undone at the top by re-exporting every one of those leaves' modules whole.

An earlier decision record permitted this: a refactor track's "pre-1.0 with
only in-repo consumers" escape hatch, which expires at 1.0. Past that point,
freezing the crate graph by default — because nobody made a decision — is
itself a decision, and the wrong one: it re-attaches at the top exactly the
constraint the leaf-crate split existed to remove.

**Decision.** Narrow the library. Delete the fourteen whole-module
re-exports:

- From `bynk-check`: `actors`, `builtin_names`, `checker`, `expr_types`,
  `firstparty`, `hints`, `index`, `kernel_methods`, `locals`, `requirements`,
  `resolver`, `store_ops`.
- From `bynk-emit`: `emitter`, `project`.

`[lib]` stays. `cargo install bynkc` is unaffected — no item re-export moves.
`bynkc`'s own integration tests, the only in-repo code that reached through
these fourteen paths, move to importing the leaf crates directly
(`bynk_check::checker::Ty` rather than `bynkc::checker::Ty`) — the correct
import in any case, since these are leaf-crate types re-exported only for a
pre-split-era path.

Two rejected alternatives. `publish = false` would break `cargo install
bynkc`, which a prior decision preserves deliberately for CI/build
determinism. Deleting `[lib]` entirely would break the integration test
suite's namespace, and the surviving ~30 item re-exports (`CompileOptions`,
`compile_project`, `BuildTarget`, `strip_project_to_js`, the diagnostic
renderers, …) are a small, legitimate public API for a published compiler
library — there is nothing wrong with `bynkc` having *some* public surface,
only with that surface being "every module of two internal crates."

Why this and not more: the `bynk` driver relates to `bynkc` the way `cargo`
relates to `rustc` — the driver is the everyday front end, and the compiler
stays independently installable, which is exactly what CI/build determinism
wants from a pinned compiler binary with no orchestration dependency. And
once the module re-exports are gone, deleting `[lib]` altogether — if that is
ever wanted — is a small, independent step; nothing here forecloses it.

**Consequences.** `bynkc`'s public API keeps its item re-exports and loses
the fourteen whole-module ones; `bynkc::checker::Ty`, `bynkc::resolver::*`,
`bynkc::emitter::*`, and the other eleven module paths stop resolving for any
external consumer. `cargo install bynkc` and `bynkc::compile(...)` (the only
usage the workspace's fuzz target makes of the crate) are unaffected. A
narrower `bynk build` verb on the driver — with `bynkc` receding further
toward the CI/build-determinism role the cargo/rustc analogy already
reserves for it — is a plausible later step but user-facing CLI surface, a
different category of change from this one; nothing here depends on it or
forecloses it.
