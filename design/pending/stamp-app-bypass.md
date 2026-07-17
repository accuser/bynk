---
level: minor
changelog: The stamp workflow pushes as a GitHub App with ruleset bypass — `main` is protected, so the direct GITHUB_TOKEN push ADR 0206 assumed cannot work (amends ADR 0206)
---

## ADR: stamp-pushes-as-a-github-app
title: The stamp pushes as a GitHub App with ruleset bypass — correcting ADR 0206's "main is unprotected" premise
summary: `main` is ruleset-protected; the stamp mints a GitHub App token (in the ruleset bypass) and pushes as the App, not the GITHUB_TOKEN
status: Accepted

**Amends [ADR 0206](0206-allocation-on-main.md) Decision 4.** The rest of 0206 stands — the pending-increment format, the `cargo xtask stamp` command, per-merge-not-batched, and delete-what-you-consume idempotency are unchanged. Only *how the stamp commit reaches `main`* is corrected.

## Context

ADR 0206 D4 stated: "`main` is unprotected, so the default `GITHUB_TOKEN` pushes the stamp commit directly," and leaned on the property that a `GITHUB_TOKEN` push triggers no further workflow run as its loop prevention. **That premise was factually wrong.** `main` is protected by a repository **ruleset** (`main protection`, created 2026-06-08) that requires a pull request and the "CI green" status check. The error was a wrong check: `GET /repos/…/branches/main/protection` (the *classic* branch-protection API) returns **404 even when a ruleset protects the branch** — rulesets are a separate surface (`GET /repos/…/rules/branches/main` shows the rules that apply; `…/rulesets/<id>` shows bypass actors).

The first live stamp run confirmed it: on the merge of #698 the stamp ran, computed the version + ADR correctly, and self-validated — but the `GITHUB_TOKEN` push was rejected (`GH013`: "Changes must be made through a pull request"). It failed safe (pushed nothing; `main` untouched; the pending file survived), and the increment was recovered by a manual `cargo xtask stamp --apply` landed as a normal PR (#699).

## Decision

**The stamp pushes as a GitHub App whose identity is in the `main protection` ruleset's bypass list** — exactly the fallback 0206 D4 named for a protected `main`, now the mechanism rather than a fallback. The workflow mints an installation token (`actions/create-github-app-token`, from secrets `STAMP_APP_ID` / `STAMP_APP_KEY`), checks out and pushes with it, and attributes the commit to the App's bot identity. The App bypasses the pull-request + status-check rules; no other pusher may write to `main` directly.

Rejected again, and why: adding the generic *GitHub Actions* bot to the bypass would widen it to every workflow, not just the stamp; a stamp-PR-with-auto-merge would need a non-default token to trigger CI anyway and adds a PR + full CI per increment. A dedicated App scopes the bypass to one auditable identity.

## Consequences

- **A new dependency**, inverting 0206's: the mechanism no longer depends on `main` being *unprotected* — it now depends on the ruleset staying in place **with the App in its bypass list**, the App installed with `contents: write`, and `STAMP_APP_ID` / `STAMP_APP_KEY` present as repo secrets. Absent any of these, the stamp fails safe (nothing pushed; pending file survives; recover with a local `cargo xtask stamp --apply`).
- **Loop prevention changes.** A `GITHUB_TOKEN` push triggered no workflows; an **App push does** — so the stamp commit now runs the normal CI on `main` (a bonus: the stamp commit is CI-validated on `main`, not only self-validated in the job) and re-fires this workflow. The re-run is skipped by a job-level guard on the stamp commit's own message (`chore(stamp):`), which replaces the free no-trigger loop prevention. The in-job self-validation is kept as a *pre-push* gate (fail before landing, not after).
- **The recovery path is unchanged and remains the documented fallback** — a failed or unconfigured stamp is repaired by running `cargo xtask stamp --apply` on a branch and merging it as a PR, as #699 did.
