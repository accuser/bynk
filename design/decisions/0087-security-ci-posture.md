# 0087 — Emitted security boundaries carry a standing behavioral test; SAST + secret scanning are committed gates

- **Status:** Accepted (v0.49)
- **Spec:** none (CI/tests posture; no language surface)
- **Builds on** ADR 0076 (verification-bearing slices gate on `/security-review`) and 0086 (first-party sources are real files).

## Context

The v0.45–v0.47 actors work put a real authentication boundary in the *emitted*
output: the Bearer JWT verifier. Its correctness was established by a one-time
`/security-review` (ADR 0076). But a one-time review is not a regression guard —
a later slice could reopen a bypass (`alg:none`, a fail-open path, an inverted
check) and nothing in CI would catch it, because `tsc --strict` checks types, not
that a tampered token is rejected. And the verifier was invisible to SAST while it
lived as a Rust string literal (0086 fixed the literal; this decides the tooling).

The existing supply-chain posture is already strong (`cargo audit`, `cargo-deny`,
`dependency-review`, OpenSSF Scorecard, SHA-pinned actions). The gaps were a
*behavioral* test of the emitted auth boundary, SAST, and secret scanning.

## Decision

**Emitted security boundaries carry a standing behavioral test.** The Bearer
verifier has `bynkc/tests/bearer_auth.rs`: a Node-driven test that imports the
emitted runtime and feeds it crafted JWTs, asserting the verdict for every bypass
class (tampered signature, `alg:none`, algorithm confusion, expired, `nbf`-future,
malformed `exp`, missing/empty `sub`, malformed token) **and** the accept path.
A future change that reopens a bypass fails here. This is the durable guard the
one-time review cannot be; future authenticated schemes (Signature, RS256) extend
the same test by default. The behavioral test — not SAST — is the load-bearing
guard for emitted verification code, because the verifier is generated and a SAST
tool reasons about syntax, not "does this reject a forged token."

**SAST via CodeQL** runs as a committed, SHA-pinned workflow (`codeql.yml`,
`javascript-typescript` + `rust`). It is free on this public repo and satisfies
the Scorecard SAST check. Its real coverage is the TypeScript surface (the
extension + the now-real `runtime.ts`); the Rust leg is maturing — defense in
depth over `clippy` + the type system. CodeQL reports to the Security tab and is
**not** a hard PR gate (it is not in `ci-green`'s required set) — SAST signal is
triaged, not merge-blocking, until its signal is known.

**Dependency audit parity** — an `npm audit --audit-level=high` job
(`vscode-bynk`, `tree-sitter-bynk`) joins `cargo audit` as a required gate via
`ci-green`, closing the un-audited JS dependency surface.

**Secret scanning** is **GitHub-native secret scanning + push protection** —
enabled in repository settings (free on this public repo), which *prevents* a
secret reaching the remote at push time. This is a settings control, not a
committed file. A committed `gitleaks` job (for PR-diff + history coverage,
SHA `gitleaks/gitleaks-action@e0c47f4f…` v3.0.0) is a noted follow-up — deferred
in favour of native push protection as the primary control, to avoid a
CI-unverifiable, org-licensing-sensitive gate.

## Consequences

The emitted auth boundary now regresses loudly in CI, not silently. The security
posture gains SAST (Security tab + Scorecard credit) and JS dependency auditing
as committed, SHA-pinned surface, and secret scanning as a prevent-at-push
control. The behavioral-test requirement is the durable rule: any slice that
emits verification logic adds (or extends) its bypass-class test, alongside its
`/security-review`.
