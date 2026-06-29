# In-browser Bynk — the browser platform, the JS emit path, the wasm toolchain, and the REPL/playground

- **Status:** Draft (settling). Direction not yet merged; no slice authorised.
- **Realises:** `design/bynk-design-notes.md` §18 (Tier-3 platform bindings — "other platforms have their own bindings exposing the same capability surface … the compiler injects the binding at link time based on build target"), §19 ("additional backends … leave the door open in principle"), and the §19 aside that "a REPL is ambitious and probably v2 or v3" — this track is that realisation, front-loaded by enabling slices that pay off independently.
- **Posture:** Feature track per [ADR 0076](../decisions/0076-feature-track-posture.md). Qualifies on three axes at once: multi-increment (five-plus slices), surface not yet settled (REPL UX, capability exposure), and a **safety boundary** (the REPL executes compiler output in the user's browser).
- **Deployment target:** `https://playground.bynk-lang.org` — registered, Cloudflare-hosted, not yet serving. A fully static, client-side app (client-side wasm compile, client-side eval): no server compute, so Cloudflare Pages is the natural host. See §3.5.
- **Proposed front-loaded ADRs:** 0136 (strip-only emission invariant), 0137 (first-class JS artefact), 0138 (the Browser platform), 0139 (the wasm-compiled toolchain), 0140 (the REPL execution & sandbox model). Numbers provisional; last landed ADR is 0135.

## 1. Motivation

Bynk already lowers each context to a TypeScript module and injects a per-target
platform binding at link time (§18). Two backends exist — Node (the `Bundle`
target's default) and Cloudflare (`Workers`). Nothing about the lowering is
inherently server-bound: the portable runtime speaks the **Web Fetch API**
(`fetch`/`Request`/`Response`), and the Node binding's only genuinely host-locked
dependency is `process.env` for secrets — `crypto.randomUUID()` and `fetch` are
already Web standards.

That makes a browser an attainable third target, and a browser target unlocks the
thing the design notes park at "v2 or v3": an **in-browser REPL / playground** where
a newcomer types Bynk and sees it run, with no install. For an explicitly
*educational* language, a zero-install playground is not a side-quest — it is the
single highest-leverage on-ramp, the thing a tutorial link can point at.

The work decomposes into four layers, each smaller than the last suggests, and the
lower three carry value even if the REPL itself slips:

1. **A strip-only emission invariant + a first-class JS artefact.** The emitter is
   already 99% strip-only-clean; closing the gap lets `bynkc` produce runnable JS
   without a TypeScript compiler in the loop. Useful for small deploy artefacts and
   *required* for an in-browser eval that doesn't ship a transpiler.
2. **A Browser platform** — a Tier-3 binding (§18) exposing the capability surface
   over Web APIs, composed with the existing `Bundle` topology.
3. **A wasm-compiled toolchain** — the `syntax → check → emit` pipeline compiled to
   `wasm32`, so the browser can compile Bynk, not merely run pre-compiled output.
4. **The REPL/playground** — the web shell, the sandboxed execution model, and the
   editor integration that ties the three together.

## 2. Scope and non-goals

**In scope.** A `--emit js` artefact; a `--platform browser` binding; a wasm build of
the compiler front-to-emit; a sandboxed browser REPL that compiles and runs the
*in-process subset* of Bynk (the `Bundle` topology); shareable snippet links;
syntax-highlighted editing.

**Non-goals (and why).**

- **Replacing TypeScript-first output.** TS stays the default and primary artefact.
  §19's reasons hold unchanged — typed output documents the lowering, surfaces
  emitter bugs as type errors at the runtime-library boundary, and fits the
  Cloudflare toolchain. JS is *additive and opt-in*, justified by the two cases TS
  can't serve: in-browser eval without a transpiler, and minimal artefacts.
- **WASM as program output.** Still rejected per §19. The wasm in this track is the
  **compiler's** distribution form (Rust → `wasm32`), not the Bynk program's. The
  program still lowers to TS/JS. These are orthogonal uses of the word; the doc is
  careful to keep them apart, and the §19 commitment is untouched.
- **Running the `Workers` topology in-browser.** Durable Objects, Service Bindings,
  cross-context wire calls, and WebSocket shapes (`from websocket`, the hibernation
  and broadcast paths landed by the websocket track) have no browser analogue. The REPL targets the
  `Bundle` (single in-process bundle) subset; programs that reach Workers-only
  shapes are *diagnosed as unsupported-in-browser* using the existing platform-lock
  machinery, not silently broken.
- **A production browser deployment target for real apps.** The Browser platform
  exists to serve the playground and education, matching §19's framing — not to host
  user-facing applications. Porting the Cloudflare-leaning shapes (§18) wholesale is
  explicitly out of scope.

## 3. Internal architecture

### 3.1 Layer 1 — strip-only invariant and the JS artefact

The emitter emits, across its whole surface, only TypeScript that pure
*type-stripping* removes: `export type` aliases, `interface`s, `const` objects,
classes, and type-annotated members. An audit found **exactly one** type-directed
construct — a parameter property at `bynk-emit/src/emitter/emit.rs:685`:

```rust
writeln!(out, "  constructor(private deps: {{ {deps_ty} }}) {{}}").unwrap();
```

`private deps` relies on the runtime synthesising `this.deps = deps`, which pure
stripping does not do (Node's `--experimental-strip-types` throws
`ERR_UNSUPPORTED_TYPESCRIPT_SYNTAX` on it). The fix is to de-sugar it
**unconditionally** — not to branch per target — keeping the field typed for the
`tsc`/strict path:

```rust
writeln!(out, "  private deps: {{ {deps_ty} }};").unwrap();
writeln!(out, "  constructor(deps: {{ {deps_ty} }}) {{ this.deps = deps; }}").unwrap();
```

Because the emitted tsconfig targets **ES2022**, `useDefineForClassFields` is on:
the declared field defines `this.deps` at construction, then the body assigns the
real value; end state is correct, and the form strips cleanly to
`constructor(deps) { this.deps = deps; }`. This is the only site, so the rewrite
*removes* a special case rather than adding a branch — and it repairs an existing
latent inconsistency: the `ImportExt::Ts` debug path (`bynkc test --inspect`) claims
to run emitted `.ts` under Node strip-only, yet currently emits this unstrippable
construct for providers with a `given` clause.

With strip-only total, a JS artefact is *emit-then-strip*: the same emitter output
with annotations elided. A regression test asserting "all emitted TS is
strip-removable" makes the invariant load-bearing and guards future emitter work.

### 3.2 Layer 2 — the Browser platform

The host axis is the `Platform` enum in `bynk-check/src/firstparty.rs` (`Cloudflare`,
`Node`), each mapped to a binding module via `bynk_binding_filename()` /
`bynk_binding_source()` (an `include_str!`'d
`bynk-check/src/firstparty/bindings/bynk-<platform>.ts`) and a
stable `as_str()` name. A Browser platform is:

- `Platform::Browser` + `as_str() => "browser"` + `--platform browser` wiring;
- `bynk-check/src/firstparty/bindings/bynk-browser.ts`, a Tier-3 binding implementing the same capability
  surface as the Node binding (`Clock`, `Random`, `Logger`, `Fetch`, `Secrets`)
  over Web APIs. The Node binding already uses `crypto.randomUUID()` (Web Crypto)
  and `fetch`/`Request`/`Response` (Web standards) unchanged; the one substitution
  is `Secrets` (no `process.env` in a browser — see §4);
- composed with `BuildTarget::Bundle`. A browser cannot do the `Workers` wire-call
  model, so the topology is fixed to `Bundle`; the Workers coupling is localised to
  the Workers/WebSocket emitter modules (`emitter/workers*.rs`, `emitter/wrangler.rs`,
  and now `emitter/websocket.rs` — the latter added by the websocket track, with
  incidental touches in `lower.rs`/`serialisation.rs`/`runtime.ts`), so a Bundle
  build sidesteps it.

Dependency policing comes for free: `platform_of()` / `lock_violation()` already
reject platform-native units against the wrong platform, so a browser build refuses
anything that pulls in `bynk.cloudflare` (or a future Node-locked unit) **at validate
time** — which is exactly how the REPL surfaces "this program uses Workers-only
shapes" rather than failing at runtime.

### 3.3 Layer 3 — the wasm toolchain

The REPL needs the compiler *in the browser*, not the program-as-wasm. Compile the
`bynk-syntax → bynk-check → bynk-emit` pipeline to `wasm32-unknown-unknown` via
`wasm-bindgen`, exposing one entry point:

```
bynk_compile(source: &str, opts: CompileOpts) -> { js: Option<String>, diagnostics: Vec<Diagnostic> }
```

The crates are largely portable; the one snag is that `bynk-emit`'s
`project.rs`/`lib.rs` touch `std::fs` for project discovery. The wasm entry feeds the
emitter an **in-memory single-module project**, bypassing filesystem discovery — a
clean seam to carve, and the same seam a future "compile a virtual multi-file project"
mode would reuse. Rust-in-wasm for a compiler is well-trodden (swc, Biome,
rust-analyzer). The runtime library (`runtime.ts`) and the browser binding ship as
strings alongside the wasm so the eval step has no network dependency.

### 3.4 Layer 4 — the REPL and execution sandbox

```
┌─ editor (CodeMirror/Monaco; tree-sitter-bynk → web-tree-sitter for highlighting) ─┐
│                                   source                                          │
│                                     ▼                                             │
│                       bynk_compile (wasm)  →  diagnostics ──► inline gutter        │
│                                     ▼ js                                           │
│        link: js + runtime.ts(stripped) + bynk-browser binding(stripped)           │
│                                     ▼                                             │
│            sandboxed execution: cross-origin iframe ⟶ Web Worker (timeout)         │
│                                     ▼                                             │
│                          captured Logger output / Result value                    │
└───────────────────────────────────────────────────────────────────────────────────┘
```

The compile step is synchronous wasm. Execution runs in a **cross-origin sandboxed
iframe hosting a Web Worker** (§4, with concrete origins in §3.5): no DOM, no
parent-origin access, hard wall-clock timeout via Worker termination. The REPL scope
is the `Bundle`/in-process subset — free functions, type/refined/record/sum
declarations and their checks, capability calls against the browser binding's default
providers. Agent/DO/Workers/WebSocket-specific declarations (including `from websocket`)
are reported through the existing platform-lock diagnostic as unsupported-in-browser.

### 3.5 Hosting and origins

The playground is a fully static client-side application — wasm compile and JS eval
both run in the browser, with no server-side compute — so it deploys as static
assets to **Cloudflare Pages** at `https://playground.bynk-lang.org` (registered,
Cloudflare-hosted, not yet serving). The wasm blob and the bundled runtime/binding
strings are immutable, cache-friendly assets behind Cloudflare's CDN; the wasm is
lazy-loaded (Q3) so first paint isn't blocked on it. No Worker/Functions backend is
required for the core experience; one would only enter the picture for an *optional*
egress proxy (Q2) or share-link persistence (slice 5).

The §4 sandbox wants the executing iframe on an origin distinct from the app's, so a
hostile snippet can never reach the app origin's storage or cookies. Two options:
(a) `sandbox="allow-scripts"` **without** `allow-same-origin`, which already yields a
unique opaque origin and may suffice; (b) defence-in-depth — serve the iframe
document from a separate hostname (e.g. `sandbox.bynk-lang.org`, a second Pages
project) so even a sandbox escape lands on a bare origin with nothing on it.
*Leaning:* (b); a second Pages project is near-free and removes the app origin from
the blast radius entirely. Decided in ADR 0140.

## 4. Security and threat model

The REPL executes compiler output as live JavaScript in the user's browser; shared
playground links execute *other people's* Bynk. This is the track's safety boundary.

- **Untrusted code execution.** Treat all REPL input — typed or link-borne — as
  untrusted. Isolate in a **cross-origin** sandboxed iframe (`sandbox="allow-scripts"`,
  served from a distinct origin — see §3.5 for the `playground.bynk-lang.org` /
  `sandbox.bynk-lang.org` split) wrapping a Web Worker. No DOM handle, no
  `localStorage`/`cookie` access to the app origin, no parent messaging beyond a
  structured-clone result channel.
- **Resource exhaustion.** An infinite loop or runaway allocation must not wedge the
  tab. The Worker runs under a wall-clock budget and is `terminate()`d on overrun;
  the UI reports a timeout. (Memory caps are best-effort in-browser — see Q3.)
- **Network egress.** The `Fetch` capability is the sharp edge: arbitrary outbound
  requests from the playground origin invite SSRF/exfil-by-proxy and abuse. **Default
  posture: `Fetch` is withheld** in the public playground binding (calls return a
  capability-unavailable error); an opt-in "advanced/local" mode may enable it, or a
  same-origin allowlisted proxy. Decided in ADR 0140 / Q2.
- **Secrets.** The browser binding's `Secrets` provider has no `process.env`. It
  resolves to *unavailable* (not a silent empty), so programs depending on secrets
  fail loudly and educationally rather than appearing to run with blank values.
- **Supply chain of the eval bundle.** `runtime.ts` and the binding are shipped as
  vetted, pre-stripped strings bundled with the wasm — not fetched at eval time — so
  there is no third-party script-injection surface in the hot path.

## 5. Open questions (settle before slicing)

- **Q1 — JS production route. [DECISION]** Built-in strip pass in `bynk-emit` vs
  reuse the existing `tsc → out-js` route vs ship a pre-stripped runtime only.
  *Recommendation:* a built-in strip pass, so neither `bynkc --emit js` nor the
  browser has any Node/`tsc` dependency. (The `tsc` route stays available for users
  who want type-checked JS.)
- **Q2 — Browser `Fetch` exposure. [DECISION]** Withhold by default / same-origin
  allowlisted proxy / opt-in real fetch. *Recommendation:* withhold in the public
  playground; revisit a proxied form once there's demand.
- **Q3 — wasm payload budget. [DECISION]** Ship `bynk-check` (diagnostics are the
  point of a REPL) vs emit-only to shrink the bundle. *Recommendation:* ship check,
  measure, then squeeze with `wasm-opt` + thin LTO; lazy-load the wasm.
- **Q4 — editor integration.** Reuse `tree-sitter-bynk` via `web-tree-sitter`
  (already a tree-sitter grammar in-repo) for highlighting now; LSP-in-browser
  (the `bynk-lsp` server over wasm) is a later, optional enhancement.
- **Q5 — REPL granularity.** Whole-program (a single in-process unit) vs
  expression-level snippets with an implicit wrapper. *Leaning:* whole-unit, with a
  starter template, to keep the mental model honest to Bynk's structure.
- **Q6 — runtime/binding linking in-browser.** Confirm the stripped `runtime.ts` +
  `bynk-browser.ts` compose with emitted JS under a single module graph the iframe
  can import (e.g. via blob-URL ES modules) with no bundler.

## 6. Slice decomposition (ordered)

Each slice is an ordinary `vX.Y-<slug>.md` proposal citing this doc and its ADRs;
merging the proposal authorises the build. The first three slices stand on their own.

- **Slice 0 — strip-only invariant.** De-sugar the parameter property
  (`emit.rs:685`) unconditionally; add the "all emitted TS is strip-removable"
  regression test; fix the `test --inspect` strip-only path. Lands ADR 0136.
  Smallest, lowest-risk, value independent of everything below.
- **Slice 1 — first-class JS artefact.** `bynkc compile --emit js` via emit-then-strip
  (Q1). Lands ADR 0137.
- **Slice 2 — the Browser platform.** `Platform::Browser`,
  `bynk-check/src/firstparty/bindings/bynk-browser.ts`
  (Clock/Random/Logger over Web APIs; Fetch per Q2; Secrets withheld),
  `--platform browser`, platform-lock wiring; `Bundle` topology only. Lands ADR 0138.
- **Slice 3 — the wasm toolchain.** Decouple project discovery from `std::fs` (the
  in-memory project seam); `wasm-bindgen` `bynk_compile` entry returning `{ js,
  diagnostics }`; size budget per Q3. Lands ADR 0139.
- **Slice 4 — the REPL shell.** Web UI, the iframe+Worker sandbox, wasm-compile →
  link → eval, diagnostics surfacing, the platform-lock "unsupported-in-browser"
  path. Lands ADR 0140 (the safety boundary).
- **Slice 5 — playground polish (deferred/optional).** Shareable links, an examples
  gallery, persistence, and LSP-in-browser (Q4). Cut once the base proves out.

## 7. Risks

- **wasm payload size.** Shipping `bynk-check` may be heavy. *Mitigation:* measure
  early (slice 3), `wasm-opt`/LTO, lazy-load; emit-only fallback exists if needed.
- **Subset confusion.** Users may expect agents/Workers/WebSocket shapes to run in the
  playground. *Mitigation:* the platform-lock diagnostic names the limitation
  precisely at compile time; the playground UI states the in-process scope up front.
- **Strip-only as a standing constraint.** The invariant forbids future
  type-directed emitter constructs (enums, namespaces, parameter properties,
  decorators) forever. *Mitigation:* the slice-0 regression test makes any violation
  a failing build rather than a latent runtime break.
- **Third-binding parity drift.** A Browser binding is a third surface to keep in
  lockstep with Node/Cloudflare. *Mitigation:* the capability surface is small and
  shared; a conformance test across bindings.
- **Egress abuse via the playground origin.** Addressed by the §4 default-withhold
  `Fetch` posture; revisit only behind a proxy.

## 8. Relationship to the north star

This track walks through a door §18–§19 deliberately left ajar: per-platform Tier-3
bindings "exposing the same capability surface," "additional backends … the door
open in principle," and a REPL flagged for v2/v3. It contradicts none of §19's
commitments — TypeScript stays primary; JS is additive; WASM remains rejected *as
program output* and is used here only as the compiler's distribution form. The
payoff is the educational on-ramp the design notes have always pointed at: type
Bynk, press run, in a browser, with nothing installed.
