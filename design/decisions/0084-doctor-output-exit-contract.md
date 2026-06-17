# 0084 — The `karn doctor` output and exit-code contract

- **Status:** Accepted (v0.46)
- **Realises:** v0.46 `karn doctor` proposal.
- **Relates:** [[0083]] (the `karn` driver), [[0071]] (`karnc check --format short`).

## Context

`doctor` answers: *given what you want to do with Karn, is your machine ready,
and if not, what do you run to fix it?* The exit code is the scriptable surface
— CI reads it — so it must be defined, not implicit. The trap is a flat
pass/fail over every conceivable tool: it punishes a compile-only user for a
missing `wrangler` and trains people to ignore the output.

## Decision

Probes are **grouped by the capability they unlock**, not listed flat:

- **compile / check / fmt** — `karnc` itself (plus the skew check of [[0083]]).
  Always satisfiable when `karnc` resolved; this is the *compile floor*.
- **`karn test`** — Node **and** one of `tsc`/`tsx` (the runner ladder).
- **dev / deploy** — Node **and** `wrangler`.
- **editor** *(optional)* — `karnc-lsp`. Missing is a **note**, never a failure.
- **build-from-source** *(optional, contributors)* — a Rust toolchain; shown
  only inside the Karn repo.

Each check reports **presence + version + provenance** (`PATH` /
project-local `node_modules/.bin` / `npx` fetch-on-demand). `npx`-provisionable
is reported as **provisionable, not present** — it must never read as a green
"ok", because `npx --yes` *downloads* on first use.

**Exit code** turns on *what the invocation asks about*:

- **Bare `karn doctor`** is *informational*. It surveys every capability but
  treats only the compile floor as required, so it exits **0** even with
  `test`/`dev` unavailable — a compile-only user is healthy. It exits non-zero
  only if `karnc` is unresolvable or majorly skewed.
- **`--only <capability>`** promotes that capability's tools to **required**:
  `--only deploy` on a machine with no `wrangler` (and no `npx`) exits non-zero.
  Spelt `--only <cap>`, not a bare `karn doctor test` positional, to avoid
  colliding with the `test` verb.
- **`--strict`** promotes *all* warnings — optional gaps, `npx` provisionability,
  minor skew — to failures, for an all-green CI gate.

**Output:** a grouped human table by default; **`--format short`** (one
`capability: level (remedy)` line) and **`--format json`** are the pinned
scriptable surface, siblings to `karnc check --format short` ([[0071]]). For
each gap `doctor` prints the exact remedy (the `npm install -g …` / `npx …`
strings the toolchain already emits) but **does not install** — `--fix` is a
different trust bar, reserved-and-noted.

## Consequences

The bare exit code is meaningful and stable (0 unless the floor is broken);
`--only`/`--strict` are how a caller asks for a hard gate. The `short`/`json`
shapes are golden-pinned — `json` is serialised from explicit structs so its
field order is deterministic regardless of `serde_json`'s `preserve_order`
feature unifying across the workspace. The human table is smoke-tested only.
