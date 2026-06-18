# 0093 — The completion surface contract: a canonical context × candidate-kind matrix, registry-sourced and coverage-tested

- **Status:** Accepted (doc-ADR; 2026-06-18)
- **Spec:** `design/karn-lsp-spec.md` §3.15
- **Realises:** the LSP tooling track (`design/tracks/lsp.md`), slice 0 (front-loaded ahead of slice 1).

## Context

Completion grew one conservative slice at a time (ADRs 0061–0064): positional
first, then name-receiver `.`, then value-receiver `.`, then locals. Each slice
settled its own contexts in isolation; **no document ever stated the whole
surface** — which symbols complete *where*, and the guarantee that *everything
which could complete in a cell does*. The cost surfaced as a cluster of gaps
(track `tracks/lsp.md` G1–G6): the `.` trigger char is unregistered (members
never auto-fire); the builtin-statics table omits `List.empty`/`Map.empty`/
`Effect.pure`; builtin sum types (`HttpResult`/`QueueResult`) complete no
variants; expression position offers locals only (no constructors, functions, or
type names); free-function/stdlib names complete nowhere. None is a missing-data
problem — every one of these is already enumerable from a `karnc` registry. They
slipped through because **there was no completeness contract and no test that
fails when the language grows past the completion wiring.**

Slice 1 (G1–G3) is meant to be a trivial data-and-config change. But "what
*should* complete at each cursor" — especially the expression-position set — is a
genuine design call that, decided ad hoc inside the trivial slice, would either
under-bake or stall it. The track front-loads that call here, as a standalone
doc-ADR, so every completion slice implements against a settled target.

## Decision

**Completion has one canonical surface: a matrix of *cursor context × candidate
kinds*. Every item offered falls in a cell; every populated cell offers
*everything* its source registry holds (modulo the clean-file ceiling, D5).**

### The matrix

| # | Cursor context | Detection | Candidate kinds |
|---|---|---|---|
| 1 | `consumes <prefix>` | lexical | **Unit** (contexts, adapters, `karn` surface) |
| 2 | `consumes U { … }` | lexical | **Capability** exported by `U` |
| 3 | `given …` | lexical | **Capability** in scope (local + `U.Cap`) |
| 4 | type position (`: T`, `-> T`, `[ … ]`) | lexical | **Type** (builtins + surface transparent + project) |
| 5 | keyword / declaration start | lexical | **Keyword** (lowercase reserved) + **Snippet** + **Variable** (locals, appended) |
| 6 | name-receiver `Upper.` | lexical + project-parse/registry | **Variant** (sum — incl. builtin `HttpResult`/`QueueResult`) · **Member** (refined/opaque `of`/`unsafe`; capability op; type static) |
| 7 | value-receiver `lower.` | overlay (ceiling, D5) | **Method** (kernel) + **Field** (record) |
| 8 | expression position | lexical + scope | **Variable** (local/param) · **Function** (free / stdlib / user) · **Constructor** (`Ok`/`Err`/`Some`/`None`/`true`/`false`) · **Type** (for a static call or record construction) |

Contexts 1–6 and the non-typed parts of 5/8 are **lexical + registry +
project-parse**; only context 7 and the *inferred-type detail* on a context-5/8
local depend on the analysis overlay.

### The load-bearing calls

- **D1 — `.` is a completion trigger character.** Contexts 6 and 7 auto-fire on
  the dot, not only on an explicit re-request. (Closes G1; a `server_capabilities`
  change, adding `"."` to the existing `[" ", "{", ","]`.)
- **D2 — one source of truth, no parallel hardcoded lists.** Every cell draws
  from a `karnc` registry, never a hand-kept duplicate that can drift: types/
  keywords/surface from `karnc::{keywords, builtin_names, firstparty}` (ADR 0061);
  kernel methods from `karnc::kernel_methods` (ADR 0063); **type statics must
  cover the full real set** — `Int.parse`/`Float.parse`/`Json.encode`/`decode`
  **and** `List.empty`/`Map.empty`/`Effect.pure` (closes G2); **builtin sum
  variants** come from the AST variant tables (`http_variant`/`queue_variant`),
  on the same name-receiver path as project sums (closes G3); **free/stdlib
  functions** from the `firstparty` sources keyed off the in-scope `uses` set plus
  the current unit's `fn` exports (closes G5).
- **D3 — the expression-position set (the G4 call).** A valid expression head is
  a *value*, a *constructor*, or a *type name* (for a static call or record
  construction). So context 8 offers exactly: in-scope values (locals, params,
  free/stdlib/user functions), the six constructor keywords, and in-scope type
  names — and **nothing type-only** (the lowercase declaration keywords stay in
  context 5). The server offers the full in-scope set; ranking is the client's
  prefix filter, per LSP convention. This supersedes the locals-only stopgap
  (ADR 0064) at expression position; locals-at-keyword-position (context 5) is
  unchanged.
- **D4 — the ceiling is confined to context 7 (and local type-detail).**
  Contexts 1–6 and 8 are registry/project-parse only and **must not** inherit the
  clean-file ceiling (ADR 0063) — they offer their candidates even in a file with
  errors. The ceiling is an *architectural boundary*, not a global gate: G1–G5
  are ceiling-free by construction, and only G6 (context 7 + richer local types)
  works inside it. A slice that makes a ceiling-free cell depend on a clean parse
  is a contract violation.
- **D5 — completeness is contract-tested.** A registry-driven test enumerates
  `karnc::{keywords, builtin_names, kernel_methods, firstparty}` and the AST
  sum-variant tables, and asserts each entry surfaces in its matrix cell's
  `complete()` (or value-member) output. **Adding a base type, keyword, kernel
  method, static, or stdlib function to the language must appear in completion or
  the test fails** — the standing guard whose absence let G1–G5 accrue silently.
  It mirrors the existing `kernel_registry` / `keywords_reference` drift guards
  and is the test slices 1–4 extend per cell.

## Consequences

Slices 1–4 now implement against a fixed target instead of re-deciding the
surface each time. Slice 1 (G1–G3) reduces to "register `.`, fill the
statics/sum-variant cells, add the coverage test" — trivial, as intended, because
the design is pre-made here. Slice 2 implements D3 (context 8); slice 3 the
free-function cell (D2/G5); slice 4 lifts the ceiling for context 7 (D4's one
overlay-gated cell). The coverage test (D5) is a new maintenance point — which is
the point: drift becomes a loud CI failure, not a silently-narrow menu.

Accepted costs, carried forward: the ADR 0061 record-construction false positive
(`Order {` lexically equals a field-type position) persists; expression-position
completion (D3) is deliberately broad, leaning on client-side filtering rather
than server-side narrowing; and context 7 stays empty in a broken buffer until
G6. The matrix is the surface's single source of truth — a later context (e.g.
match-arm patterns, `assert` in tests) is an *amendment to this ADR with a new
row*, not an ad-hoc addition.
