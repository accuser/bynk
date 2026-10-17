---
level: patch
changelog: `bynk deploy --context` now also refuses a callee whose new build no longer provides the contract a live caller was compiled against (`bynk.deploy.contract_skew`), naming each caller, the service and both hashes, before the plan and under `--dry-run`; `bynk.deploy.lock` records each Worker's `expects` beside what it provides, and a caller recorded before that is warned of, not refused (#2077).
---

## ADR: callee-side-contract-skew-gate
title: The deploy contract-skew gate checks the callee side too, under the same code
summary: `--context` holds the pushed Worker's new `provides` to every other live Worker's recorded `expects`; an unrecorded caller warns

**Context.** ADR 0200 Decision H extended `deploy --context`'s D4 gate from
"the dependency exists" to "the dependency matches": the pushed Worker's
compiled `expects` against its dependencies' recorded `provides`. Nothing held
the pushed Worker's new `provides` against what its live callers were compiled
against, because the ledger recorded only `provides`. So pushing a callee whose
contract changed went ahead, and every call from the live caller was then
answered `409 ContractMismatch` in production (#2077).

**Decision.** The ledger records each Worker's `expects` (dependency →
service → hash, from its `bynk-contracts.json`) beside its `contracts`, as an
additive, `default`ed `Option` field with the same `None`/`Some({})` meaning.
Under `--context`, after the caller-side check and before the plan (so
`--dry-run` included), the pushed Worker's new `provides` is compared with every
other live Worker's recorded `expects` in the same environment. A changed hash or
a removed service is refused under the existing `bynk.deploy.contract_skew`,
naming each live caller by its deployed name, the contract and both hashes, with
the same remedy: deploy the whole project, repeating `--env`.

The same code, not a sibling: the failure (a live caller answered 409) and the
remedy (move both sides together) are the same, and one `bynk explain` entry
covers both directions. `bynk.deploy.foreign_contract_skew` is a sibling because
its remedy differs (regenerate the client, update the pin).

A live caller whose record predates `expects` cannot be checked. Silence is not
a match, so it is not refused; when this build's service graph shows it binding
to the pushed Worker, `deploy` warns that it could not check it and that a
whole-project deploy records it.

**Consequences.** A whole-project deploy is unaffected: the gate is a
`--context` gate, as D4 is, and a whole-project run moves the callers with the
callee. A live caller this build no longer has, a Worker of a removed context
still recorded in the ledger, is refused under `--context` too, but with a
different remedy. A whole-project deploy reports it as `orphan worker` without
redeploying or checking it, and `--prune` never deletes a Worker or its record.
So the refusal says to delete the Worker and remove its ledger entry, or to
restore its context and deploy the whole project. A whole-project run itself
still doesn't check such a Worker. Foreign callers keep their own pin check, which already runs
on the callee's build. The runtime 409 remains the backstop for a push behind
the driver's back or a drifted ledger.
