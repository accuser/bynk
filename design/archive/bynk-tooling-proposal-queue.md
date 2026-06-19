# Bynk tooling — proposal queue

The concrete backlog of *subsequent* tooling proposals, ordered by recommended sequence. A planning
reference (complements `bynk-tooling-roadmap.md` and `bynk-cicd-roadmap.md`); each line becomes a
`design/proposals/vX.Y-*.md` when scheduled. Sizes are rough; "gated" notes a prerequisite.

**Status @ v0.43 (2026-06-16):** the v0.30–v0.43 line shipped nearly the whole original queue —
comprehensive completion, signature help, call hierarchy, implementation navigation, folding/selection
ranges, the inlay-hint follow-ups, the InRange quick-fix, B-2 extension polish, and (CI Tier 4) the
supply-chain posture are all delivered. What remains is a short tail of deferred slices plus the
credential-blocked Tier 4 publishing. The user-facing feature line is essentially complete; this queue
is now small.

## Shipped (for context)

The A/B-tier LSP arc, with the version and ADR each item landed under:

- **B‑0** server provisioning → **A‑0** project diagnostics (v0.24, ADR 0052) → **A‑0** reference index +
  references/rename (v0.25, ADR 0053) → **A‑1** code actions + workspace‑symbols/document‑highlights
  (v0.26, ADR 0054) → **A‑2** inlay hints (v0.27, ADR 0056) → **A‑2** semantic tokens (v0.28, ADR 0057)
  → **B‑1** surface‑the‑features theming (v0.29, ADR 0058).
- **Comprehensive completion** (queue item 1) — positional (v0.30, ADR 0061), name‑member (v0.30.1,
  ADR 0062), value‑member (v0.30.2, ADR 0063), locals (v0.31.2, ADR 0064).
- **Locals navigation / semantic tokens** (part of item 3) — v0.31 / v0.31.1 (ADR 0064): references,
  go‑to‑definition, document‑highlight, and colouring for `let`/`let <-`/param bindings.
- **Signature help** (item 2) — name callees (v0.32, ADR 0065), value receivers (v0.32.1).
- **CodeLens reference counts** (part of item 4) — v0.33 (ADR 0066), + the click‑arguments fix (v0.40.1).
- **Call hierarchy** (item 5) — v0.34 (ADR 0067); method edges added with member indexing (v0.36).
- **Implementation navigation** (part of item 6) — v0.35 (ADR 0068): a capability → its provider(s).
- **Member index kinds** (part of item 3) — methods (v0.36, ADR 0069), record fields + capability ops
  (v0.36.1): go‑to‑definition, references, rename, and semantic‑token colouring extend to members.
- **Folding & selection ranges** (item 7) — v0.37 (ADR 0070).
- **B‑2 extension polish** (item 13) — snippets/scaffolding/walkthrough (v0.38, ADR 0071), problem‑matcher
  + build task backed by `bynkc check --format short` (v0.38.1).
- **Richer inlay hints** (items 8 + 10) — parameter‑name hints (v0.39, ADR 0072 slice 1),
  generic‑instantiation hints (v0.39.1, slice 2).
- **InRange‑swap quick‑fix** (item 9) — v0.40 (ADR 0073).
- **Supply‑chain posture** (CI Tier 4, item 17) — OpenSSF Scorecard + all actions SHA‑pinned (#163).

**Advertised today:** hover, definition, **completion (types, fns, members, locals, keywords, snippets)**,
formatting (+range), document symbols, references, rename, code actions, inlay hints (types, parameter
names, generic instantiation), semantic tokens, workspace symbols, document highlights, **signature
help**, **CodeLens (reference counts)**, **call hierarchy**, **implementation navigation**, **folding &
selection ranges**.

---

## 1. Open tooling work (server, `bynkc` + `bynk-lsp`)

What's left is the tail of three partly‑shipped items plus two never‑started small ones.

1. **Locals‑rename + generic type parameters** — *the last unpaid slice of the recurring index deferral
   (v0.25/v0.27/v0.28/v0.31/v0.36).* Local bindings resolve and colour, but **rename** for them is still
   deferred (subtler scope/shadowing edits); **generic type parameters** are not indexed at all (no
   references/rename, no semantic‑token or inlay coverage). Also still out: match‑arm / `is`‑narrowing
   pattern bindings, and the `parameter`‑vs‑`variable` token split. *Meaty; sliceable (locals‑rename vs
   generics).*
2. **Type‑definition navigation** — `textDocument/typeDefinition`: value → its type, and a consumed
   context → its source. (The sibling, implementation navigation, shipped in v0.35.) *Bynk‑specific nav;
   index/first‑party metadata; medium.*
3. **Test‑run CodeLens** — the "▶ Run" lens above tests (the other half of item 4; the reference‑count
   half shipped in v0.33). *Gated:* needs **test discovery + a run command**. *Small once the gate lands.*
4. **`inlayHint/resolve`** — lazy hint tooltips (v0.27 out‑of‑scope; never picked up). *Small, ungated.*

## 2. Deferred optimisations (do when scale demands — premature otherwise)

5. **Semantic‑tokens `delta`** — re‑encode only changes (v0.28 deferral). *Optimisation; no scale signal yet.*
6. **Incremental recompute** — the LSP re‑runs full project analysis per debounced change; a salsa‑style
   incremental recompute for large projects. *Deferred since v0.24; do when scale demands.*

## 3. Distribution (CI/CD Tier 4 — `bynk-cicd-roadmap.md`)

7. **Extension + grammar release automation** — publish to the **VS Code Marketplace + Open VSX**;
   optionally per‑platform VSIXs **bundling `bynkc-lsp`** (the offline alternative to download‑on‑activate).
   *Gated:* **needs marketplace tokens.**
8. **Binary signing / notarisation** — macOS notarisation + Windows signing for the downloaded server
   (Gatekeeper/SmartScreen friction). *Gated:* **needs certificates.**

## Not tooling, but it gates tooling

- **`given` on free functions** (the v0.23 discovered limitation) — language‑core; until it lands, no
  capability can be driven from a recursive/factored helper, which caps what completion/codeLens
  examples and any capability‑iteration tooling can demonstrate. Tracked in `bynk-project` memory.

---

## Suggested sequence

The big interactive wins are all shipped, so what's left is paydown and polish — schedule it when a
calmer increment is wanted:

1. **Locals‑rename + generic type parameters** — closes the recurring index deferral that every
   member/locals increment has cited since v0.25; the highest‑value remaining item.
2. **Type‑definition navigation** then **`inlayHint/resolve`** — both small, both round out features
   that already ship most of their surface.
3. **Test‑run CodeLens** — once test discovery + a run command exist.

**CI Tier 4 publishing** (Marketplace + signing) slots in whenever the tokens/certs are provisioned —
independent of the feature line. The **deferred optimisations** (delta, incremental recompute) wait for a
real scale signal; building them now is premature.
