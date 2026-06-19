# 0083 — Introduce the `bynk` driver as a thin orchestrator distinct from the `bynkc` compiler

- **Status:** Accepted (v0.46)
- **Realises:** the project-lifecycle wrapper arc (`design/bynk-tooling-roadmap.md` §5.1), v0.46 `bynk doctor` proposal.
- **Relates:** [[0060]] (named concern-modules), the `vscode-bynk` `bynkc-lsp` resolution order.

## Context

Bynk shipped four binaries — `bynkc`, `bynk-fmt`, `bynk-grammar`, `bynk-lsp` —
all *compiler operations*. The lifecycle wrapper arc (`doctor` → `new` → `dev`)
is **orchestration**, not compilation: "is `wrangler` installed", "scaffold a
project", "watch and run `wrangler dev`". Putting that on `bynkc` would cement
environment orchestration inside a compiler the project has deliberately kept
pure. `doctor` is the first command of the arc and the cheapest possible thing
to stand the home up with — no language surface, mutates nothing.

## Decision

Introduce a new published crate, **`bynk`**, a thin orchestrator over `bynkc`
and the Node toolchain — `bynk` is to `bynkc` what `cargo` is to `rustc`. The
compiler stays pure (compile / check / fmt / test); the driver shells it and
reports on the environment. `doctor` is its first command; `new`/`dev` follow.

`bynk` resolves the `bynkc` it shells in this order:

1. **`BYNK_BYNKC` override** — an explicit path (the `bynk.executablePath`-style
   escape hatch). When set it wins, and a bad override surfaces rather than
   silently falling through — an override that only applied after auto-discovery
   failed would be useless.
2. **`PATH`** — a global `bynkc`.
3. **sibling-of-`bynk`** — a `bynkc` next to the running `bynk` binary (mirrors
   how `vscode-bynk` resolves `bynkc-lsp` beside itself).

Because the driver and compiler are now **separate binaries that can drift**, a
global `bynk 0.46` can shell a stale `bynkc 0.44`. `doctor` therefore reports
**driver↔compiler version skew**: patch differences are ignored (wire-compatible
under unified versioning), a **minor** drift warns (fails only under
`--strict`), a **major** drift is a contract mismatch and an error even on a bare
run. Detection is **portable** — the shared probe is backed by the `which`
crate (PATHEXT/`where`), not the Unix-only `which`-shell `bynkc` used.

The crate is split into single-concern modules per [[0060]]: `probe`
(detection), `compiler` (resolution + skew), `doctor` (capability model + exit
contract — see [[0084]]), `report` (rendering).

## Consequences

One new published binary and a release-plumbing row (built and packaged
alongside `bynkc`/`bynkc-lsp`; published after `bynkc`). The skew check is a
truth the driver owes *because* of this split — silencing it would defeat the
command. The Node floor is read from a single `bynkc::NODE_MAJOR_FLOOR` constant,
so the driver does not become a third prose reader of "≥ 18". `new`/`dev` inherit
the resolution, the portable probe, and the skew machinery for free.
