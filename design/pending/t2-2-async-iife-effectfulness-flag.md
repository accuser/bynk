---
level: patch
changelog: A value-position match/if IIFE's async-wrap decision is a flag computed during lowering instead of a scan of the generated text for the substring "await "
---

T2.2 (#1018) replaces `maybe_async_iife`'s `if !iife.contains("await ")` with
`LowerCtx::emitted_await`, a flag set at the two statement sites that actually
emit a literal `await` (`Statement::EffectLet`, `Statement::Do`) and
read-and-reset around a value-position `match`/`if` IIFE's own body
construction — `lower_match_as_iife` and `lower_if`'s non-ternary path. R6.4:
"Effectfulness is a property of the IR node, not of a scan over emitted text."

The scan over-matched: a `Query`/broadcast iterator terminal (`forEach` and its
five siblings — `parTraverse`, `traverseAll`, `parTraverseAll`, `traverseTry`,
`parTraverseTry`) lowers to a self-contained `async () => {...}` IIFE emitted as
an ordinary `Effect`-typed *value*, not something the arm itself awaits. The
scan saw the literal `"await "` inside that embedded string and wrongly wrapped
the *enclosing* switch/if arrow as async too, producing a redundant
`await await (async (__d) => …)()` where a single `await` on a synchronous
arrow was correct. The flag only becomes true where a literal `await` is
actually emitted in the current arrow's own scope, so this case no longer
wraps.

A nested match/if that genuinely needs the wrap still propagates that need
outward: `finish_async_iife`'s caller restores the flag as `saved || needs_async`,
mirroring the substring-survives-in-the-returned-string propagation the old
scan got for free. Not isolated around a lambda body — a nested effectful
lambda still marks the enclosing IIFE async, exactly as the old scan's text
match did; closing that is a separate, unscoped defect.

The track doc's T2.2 row said "the `if`-IIFE path uses it too", implying that
call was still missing; it had already landed via `8068c0db` (Wave 4), five
days before the row's own text was committed. T2.2's actual remaining scope was
only the scan-to-flag swap, corrected in the row.

No `.bynk` surface, grammar, checker, or runtime change. Every positive
fixture reproduces byte-identically (`cargo test -p bynkc --test e2e`); the two
new over-match regression fixtures (`match_arm_iterator_terminal_does_not_force_the_switch_arrow_async`,
`if_arm_iterator_terminal_does_not_force_the_iife_arrow_async`) are crate-local,
over the `emit_source` seam (`bynk-emit/src/emitter/lower.rs`), alongside two
non-regression fixtures pinning the already-fixed defect and nested-match
propagation.
