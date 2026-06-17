# 0083 — Introduce the `karn` driver as a thin orchestrator distinct from the `karnc` compiler

- **Status:** Accepted (v0.46)
- **Realises:** the project-lifecycle wrapper arc (`design/karn-tooling-roadmap.md` §5.1), v0.46 `karn doctor` proposal.
- **Relates:** [[0060]] (named concern-modules), the `vscode-karn` `karnc-lsp` resolution order.

## Context

Karn shipped four binaries — `karnc`, `karn-fmt`, `karn-grammar`, `karn-lsp` —
all *compiler operations*. The lifecycle wrapper arc (`doctor` → `new` → `dev`)
is **orchestration**, not compilation: "is `wrangler` installed", "scaffold a
project", "watch and run `wrangler dev`". Putting that on `karnc` would cement
environment orchestration inside a compiler the project has deliberately kept
pure. `doctor` is the first command of the arc and the cheapest possible thing
to stand the home up with — no language surface, mutates nothing.

## Decision

Introduce a new published crate, **`karn`**, a thin orchestrator over `karnc`
and the Node toolchain — `karn` is to `karnc` what `cargo` is to `rustc`. The
compiler stays pure (compile / check / fmt / test); the driver shells it and
reports on the environment. `doctor` is its first command; `new`/`dev` follow.

`karn` resolves the `karnc` it shells in this order:

1. **`KARN_KARNC` override** — an explicit path (the `karn.executablePath`-style
   escape hatch). When set it wins, and a bad override surfaces rather than
   silently falling through — an override that only applied after auto-discovery
   failed would be useless.
2. **`PATH`** — a global `karnc`.
3. **sibling-of-`karn`** — a `karnc` next to the running `karn` binary (mirrors
   how `vscode-karn` resolves `karnc-lsp` beside itself).

Because the driver and compiler are now **separate binaries that can drift**, a
global `karn 0.46` can shell a stale `karnc 0.44`. `doctor` therefore reports
**driver↔compiler version skew**: patch differences are ignored (wire-compatible
under unified versioning), a **minor** drift warns (fails only under
`--strict`), a **major** drift is a contract mismatch and an error even on a bare
run. Detection is **portable** — the shared probe is backed by the `which`
crate (PATHEXT/`where`), not the Unix-only `which`-shell `karnc` used.

The crate is split into single-concern modules per [[0060]]: `probe`
(detection), `compiler` (resolution + skew), `doctor` (capability model + exit
contract — see [[0084]]), `report` (rendering).

## Consequences

One new published binary and a release-plumbing row (built and packaged
alongside `karnc`/`karnc-lsp`; published after `karnc`). The skew check is a
truth the driver owes *because* of this split — silencing it would defeat the
command. The Node floor is read from a single `karnc::NODE_MAJOR_FLOOR` constant,
so the driver does not become a third prose reader of "≥ 18". `new`/`dev` inherit
the resolution, the portable probe, and the skew machinery for free.
