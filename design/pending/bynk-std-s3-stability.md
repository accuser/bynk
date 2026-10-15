---
level: minor
changelog: Standard-library items are stable or experimental, with an `[unstable] allow` opt-in in `bynk.toml`; a `match` on the extensible `FetchError` or `Method` needs a wildcard
---

## ADR: stdlib-stability-tiers
title: The standard library's 1.0 stability tiers, the `[unstable]` opt-in, and extensible enums
summary: Stable kernel and commons are Foundations-level, stable capabilities only grow, experimental items need `[unstable] allow`, and an extensible enum gains variants only because its `match`es carry a wildcard

**Context.** The 1.0 definition's Gate 1 froze the Foundations layer and said capabilities "evolve
additively", but it said nothing about the kernel and commons every Foundations program calls, nothing
about what "additively" means for an existing operation, and nothing about the platform changing
underneath a capability. The standard-library track (spine #1936) settled the tiers as D2. Two
mechanisms were missing: a way to ship an item *without* a promise, and a way to add a variant to an
exported enum. Exhaustiveness is enforced (ADR 0169), so a new `FetchError` variant would break every
user `match` that covers the enum, which would have frozen every stable error enum for all of 1.x.
Slice S3 (#1963) builds both.

**Decision.**

- **D1 — The tiers.**
  - Stable L0 (kernel) and L1 (`bynk.*` commons) items are **Foundations-level**: a breaking change
    is a 2.0 event.
  - Stable L2 (`bynk`) and L3 (`bynk.cloudflare`) items are **additive-only** through 1.x. They may
    gain items and operations. An existing one is deprecated in a minor release (an ADR 0117 warning)
    and removed or reshaped only in a major one.
  - **Experimental** items carry no promise.
  - **Platform carve-out:** a stable L3 item may break in a minor release only when the platform
    withdraws or incompatibly changes the feature underneath it, and that release's migration note
    names it. `bynk-1.0-definition.md` Gate 1 records the tiers.
- **D2 — Stability is registry data.** Each `bynk-std` `Unit` has a **required** `stability`
  (`Stable` or `Experimental`), so no entry is added without a choice. It also lists the
  `experimental` item or operation paths inside a stable unit, and its `extensible` enums. An item is
  a dotted path: a unit (`bynk.cloudflare`), an exported item (`bynk.cloudflare.Kv`), or a capability
  operation (`bynk.cloudflare.Kv.list`); an item under an experimental one is experimental. Nothing
  ships experimental yet. `bynk` lists `FetchError` and `Method` as extensible.
- **D3 — The opt-in.** `bynk.toml` gains `[unstable] allow = [...]`, and an entry covers its path and
  everything under it. Using an experimental item no entry covers is `bynk.stability.experimental`
  (an error, whose note names the entry to add). The uses checked are a `consumes` of the unit or a
  selected capability, a `uses`, an imported function called or passed as a value, and a call to a capability
  operation, flattened or qualified. An entry naming no first-party item is
  `bynk.project.unstable_unknown_item`, an error. An entry reaching nothing experimental is
  `bynk.project.unstable_not_experimental`, a **warning**, so stabilising an item breaks no build.
- **D4 — One reading for the build and the editor.** `StabilityPolicy::for_project` reads
  `[unstable]` from the overlay's `bynk.toml` when it holds one (the language server's open buffer),
  otherwise from disk. Both `run_checks` and `analyse_project` build the policy that way and report
  its manifest diagnostics, so `bynkc` and the language server never disagree about a file.
- **D5 — Extensible enums.** An extensible enum's coverage counts one more shape than its declared
  variants: a future variant that only an irrefutable pattern covers, at any depth. A `match` without
  one is `bynk.types.extensible_match_needs_wildcard`, and the wildcard, once present, is not
  `bynk.types.unreachable_arm`. Types carry no owning unit, so whether a name is an extensible enum is
  decided once per unit from its provenance and carried to every check built from it. That reaches
  handler and agent bodies, whose own inputs never carried `imported_from`. A user's own enum of the
  same name is unaffected.
- **D6 — First-party code checks under its own terms, and the default fails open.** A first-party
  unit's own `uses`, `consumes` and bodies are never refused under a project's policy; its
  dependencies are the library's business. A check input built without the project's policy (a
  suite case, the editor's recovering resolve) gets the permissive default, never the experimental
  list with an empty `allow`, which would refuse items while ignoring the opt-in.

**Consequences.**
- **Deliberate gaps:**
  - A reference to an experimental *type* is not yet checked. Types resolve without the checker's
    context (`resolve_type_ref`), and an experimental unit or capability is already refused at its
    `consumes` or `uses`. The check lands with the first slice that ships an experimental type
    inside a stable unit.
  - Suite bodies are checked under neither rule. A test is not under the compatibility promise, and
    its views lack the unit's provenance.
  - Single-file and in-memory builds, with no manifest, allow nothing.
- **Tooling:** hover marks an experimental item and an extensible enum, and completion marks an
  experimental capability or function.
- **Later slices:** a slice that ships an experimental item updates `bynk-std`'s pinned-stability
  test, so the change is visible in review.
