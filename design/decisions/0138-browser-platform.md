# 0138 — The Browser platform: a Tier-3 `bynk` binding over Web APIs, Bundle-only

- **Status:** Accepted (in-browser track, slice 2; v0.108.2).
- **Provenance:** the third slice of the in-browser track. With a JS artefact in
  hand (ADR 0137), the playground needs a *platform* whose capability surface is
  implemented over browser Web APIs. This adds `Platform::Browser` and its binding.
  **Security-bearing** (the binding is the playground's capability boundary — what
  a snippet can and cannot reach), so the track's feature-track posture applies.
- **Relation to prior records:** a third [`Platform`](0024-platform-native-via-first-party-metadata.md)
  alongside Cloudflare (v0.17) and Node (v0.18); reuses the per-deployment-unit
  platform lock ([ADR 0017](0017-platform-lock-per-deployment-unit.md)) and the
  native-platform machinery ([ADR 0024](0024-platform-native-via-first-party-metadata.md)) unchanged.
  TypeScript-first output and the `Bundle`/`Workers` topology split are untouched.

## Context

Bynk injects a per-platform `bynk` surface binding at link time; two existed
(Cloudflare, Node), near-identical because `Date.now`, Web Crypto's
`crypto.randomUUID`, `console`, and `fetch` are Web standards on both. A browser is
an attainable third target — and the prerequisite for the in-browser
REPL/playground, where the binding is also the **safety boundary**: it decides what
an untrusted snippet can reach. Two capabilities cannot be carried over naïvely —
network egress (`Fetch`) and secret access (`Secrets`).

## Decision

**`Platform::Browser` (`--platform browser`), with `bynk-browser.ts` implementing
the `bynk` capability surface over Web APIs, composed with `BuildTarget::Bundle`
only.**

- **D1 — Clock/Random/Logger are the Web-standard implementations**, byte-identical
  to the Node binding (`Date.now()`, `crypto.randomUUID()`, `Math.random()`,
  `console`). The portability claim (spec §4.2) holds across all three platforms.

- **D2 — `Fetch` is withheld and `Secrets` is unavailable; both fail loudly by
  throwing** (the playground safety boundary; track §4 / Q2). A browser *can*
  `fetch`, but arbitrary egress from the playground origin invites SSRF and
  exfil-by-proxy, so the binding refuses it; a browser has no secret store, so
  `Secrets.get` must not resolve to `None` (indistinguishable from "unset" — a
  silent way to run with blank values). Both throw a clear, educational error
  rather than degrading silently. **No `FetchError.Unavailable` variant is added** —
  that would be a breaking change to the `bynk` surface (user code matching
  `FetchError` exhaustively would stop compiling), out of scope here; throwing is
  the loud, surface-stable realisation. A same-origin proxied or opt-in "advanced"
  `Fetch` is the named follow-on (Q2 revisit at the REPL slice).

- **D3 — Browser is Bundle-only.** A browser cannot run the Workers wire-call model
  (Service Bindings, Durable Objects, cross-context wire calls), so
  `--platform browser --target workers` is rejected up front with a new diagnostic
  `bynk.target.browser_bundle_only`, before the per-unit lock below.

- **D4 — The platform lock against Cloudflare-only units comes for free.** A browser
  build that pulls in `bynk.cloudflare` is rejected **at validate time** by the
  existing native-platform machinery (`bynk.target.vendor_required`): the unit is
  native to Cloudflare, the selected platform is Browser, so the lock fires. No new
  lock logic — exactly how the REPL will surface "this program uses Workers-only
  shapes" rather than failing at runtime.

## Consequences

- A `--platform browser` build of a portable, in-process program emits
  `bynk-browser.ts` and type-checks/strips like any other platform; a positive
  fixture (`239_bynk_browser_platform`) carries it through the `tsc --strict`,
  strip-only, and JS-artefact suites, and two negative fixtures pin the
  Bundle-only and Cloudflare-lock diagnostics.
- The Browser platform exists to serve the playground and education (track §2),
  not to host real browser apps — hence the withheld egress and the Bundle-only
  constraint. The capability surface is small and shared, so the third binding
  stays in lockstep with Node/Cloudflare.
- The wasm toolchain slice (next) compiles the `syntax → check → emit` pipeline to
  wasm and pairs it with this binding to compile *and* run the in-process subset of
  Bynk in the browser.
