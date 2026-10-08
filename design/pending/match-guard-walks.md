---
level: minor
changelog: "A `match` arm's `if` guard borrows held values, and a `~>` send inside a guard threads the execution context (#1769). The linearity pass never looked inside a guard, so a guard could transfer a held connection unseen; a consuming use in a guard is now `bynk.held.consume_on_borrow`. And a send in a guard's `if` block emitted `deps.__exec.waitUntil(…)` for a context that never threaded `__exec`, so its output failed to type-check"
---

## ADR: match-guards-borrow-held-values
title: A match-arm guard borrows the held values it can see
summary: a consuming use of a held value inside a match-arm guard is bynk.held.consume_on_borrow; the guard changes no ownership state

**Context.** [[0218]] made the held-resource linearity pass govern the held values a `match` arm's pattern binds. It built each arm from the pattern and the body and never looked at the arm's `if` guard ([[0169]]). After #1760 made the shared `expr_children` iterator visit guards, this was one of the last walks that still skipped them. A guard could transfer a held value, either the arm's own pattern binding or one in scope from outside, and the pass recorded nothing. A guard is a `Bool`, evaluated before its arm is chosen, so it may run for an arm that is then not taken (#1769).

**Decision.** A guard is a **borrow scope** over every held binding it can see, in the sense of §2.9.2. The pass walks it with each owned binding lent as borrowed, then drops the guard's state. A non-consuming use is admitted. A consuming use (a transfer, `close`) is `bynk.held.consume_on_borrow`, with a note saying why. A value already consumed stays consumed, so naming it in a guard is `bynk.held.use_after_consume`. Because the borrow ends with the guard, the arm body, later arms and the unification after the `match` all see the state from before it.

**Consequences.** Source that consumed a held value in a guard compiled before and is now rejected, a breaking change for a pre-1.0 `minor`. None in this repository did. No diagnostic code is new. Today the borrow rule accepts no guard that names a held value: `Connection`'s only non-consuming operation, `send`, returns `Effect[()]`, not `Bool`. It still admits a future `Bool`-returning non-consuming operation. Rejected: (b) counting a guard's use as a transfer on the path where its arm is taken. That is unsound, because the guard has already run on the paths where the arm is not. Also rejected: (c) forbidding held values in guards outright. That is the borrow rule minus its non-consuming uses, at the cost of a new diagnostic. The type-system spec records the rule in §2.9.5.
