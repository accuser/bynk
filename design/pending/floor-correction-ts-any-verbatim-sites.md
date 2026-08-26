---
level: patch
changelog: Design pass — names the true floor for `ts_any` (a 7-file, 30-site distribution across four buckets, not the stale 2–3-site estimate) and `verbatim_sites` (an argued floor of at least 2, not 0 — three of the five sites are an unscoped signature capstone on `emit_project`/`emit_test_module`/`emit_integration_module`, not residue, with its own unnamed source-map prerequisite)
---

## ADR: ts-any-verbatim-sites-floor-correction

title: The floor for `ts_any` and `verbatim_sites` is argued, not zero — a real signature capstone and a 7-file `ts_any` distribution, neither previously named

summary: `verbatim_sites` is structurally pinned above 0 by three orchestrator functions that still return `String` at their own top level regardless of internal conversion progress; `ts_any`'s 30 sites split across seven files (not six) into four buckets, only two of which close via existing work

**Context.** ADR-C (§3.3) named a 2–3-site residual for `ts_any`, deferred to R7.7's runtime-typing
work, as the only open gap in "eliminated in full." §5 states `verbatim_sites` retires at 0. Both
predate every Arc C slice and Arc E's own design pass (#1422); a fresh, direct read against the
gated probes' own predicates finds a materially different shape for both.

`verbatim_sites` has exactly 5 real construction sites (`project.rs:1285/2480/2509`,
`tests_emit.rs:131/282`). Two are genuinely permanent (foreign adapter content; a committed runtime
build artifact, both already argued at Arc C slice 5). The other three are not residue: `emit_project`,
`emit_test_module`, and `emit_integration_module` all still return `String` at their own top level,
even though their internals already build real `bynk_ts` nodes for every part Arc C has converted —
the accumulator never distinguishes "printed from a real node" from "still raw text." No slice this
track has scheduled touches this; it needs its own capstone (convert these three signatures to
return `Vec<TsStmt>`/`TsProgram`, print once at the boundary), gated on Arc E's steps 1–6 landing,
a per-splice-point representation for `emitter/lower.rs`'s permanently-opaque output (ADR 0391),
and a source-map rebuild at the printing boundary — all three functions' source maps are built from
byte offsets into the accumulating `String` today, which node-construction time has no equivalent
of.

`ts_any` has 30 real sites (re-verified directly against `line_violates_ts_any`, not raw `grep`)
across seven files, not the six a first pass found: `serialisation.rs` (2, closes via Arc E's own
future slices), `emitter/lower.rs` (6 — 4 real collection-kernel element-type candidates plus 2
already-argued P7.2-deferred sites, not one undifferentiated group), `emitter.rs` (1, a previously
unnamed P7.2-deferred narrowing), `workers.rs` (4 — 1 ambient Durable Object stub type plus 3
cross-context dispatch casts), `project.rs` (2, the same cross-context dispatch pattern),
`project/tests_emit.rs` (11, generated test-harness/property-scaffold surface), `emitter/emit.rs`
(4, property-test driver/replay machinery).

**Decision.** Four buckets replace the single "2–3-site residual" framing: (1) 2 sites close as a
byproduct of Arc E, no separate work; (2) 4 sites (`lower.rs`'s own collection-kernel sub-group
only) are a real, small, independent slice candidate pending a spike into whether the lowering has
the real element type in scope; (3) 23 sites (the honest majority, including `lower.rs`'s other 2,
each already carrying an in-place deferred-reason comment) are asserted dynamic-by-nature or
already-argued-for-a-different-reason but not yet formally collected — each needs its own
file:line-grounded "why this stays `any`" case before R7.1 can honestly read "eliminated, named
residual"; (4) 1 site (`workers.rs`'s Durable Object stub) is recommended for exclusion as an
ambient third-party type, the same footing as `adapter_bindings`'s foreign content. This pass does
not itself close any bucket or argue bucket 3's 23 sites individually — it authorises the shape of
two follow-on efforts (a `lower.rs` kernel spike; a
residual-argument writeup) and schedules the `verbatim_sites` capstone after, not before, Arc E's
steps 1–6 land.

**Consequences.** `design/tracks/the-typescript-tree.md` §5's `verbatim_sites` bullet is corrected
from "retires at 0" to an argued floor. §6 gains a "Floor correction" section recording both
distributions and the four `ts_any` buckets, so a future retirement review does not rediscover
either as a surprise, and so a future slice proposal against `lower.rs`'s collection-kernel bucket
is sized against the confirmed 4 (not the file's own total of 6, 2 of which are a different,
already-argued kind of residual), and against `emitter.rs`'s own distinct 1-site presence, neither
previously named.
