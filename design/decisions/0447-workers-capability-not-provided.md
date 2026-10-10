# 0447 — A required capability needs a provider on the Workers target only

- **Status:** Accepted (v0.317)

**Context.** A context may declare a capability, require it with `given`, and provide it nowhere. On the bundle target that is a working pattern: the context exports `__makeSurface(deps)`, and the host passes the capability in. On Workers there is no host to pass it: the generated composition root builds every capability its handlers are given from the context's providers, so a missing one became `{}` where the capability was required, and the Worker failed `tsc` (TS2345) while `bynkc check` and the bundle build passed (#1822).

**Decision.** On the Workers target, a capability the context declares and requires, with no provider in the context, is `bynk.capability.not_provided`, reported once per capability at its first `given`. It is checked by a Workers-gated project phase, beside `phase_secrets_computed_name`. A capability the context exports is left to `bynk.exports.capability_not_provided`, and one from another unit (flattened or qualified) to that unit. Rejecting it on every target was considered and set aside, because existing bundle programs rely on the host supplying the capability.

**Consequences.** The two targets now disagree by design about whether such a program is complete, and the diagnostic says so, pointing at `--target bundle`. `bynkc check` without a Workers target doesn't report it. A Workers build or deploy does.
