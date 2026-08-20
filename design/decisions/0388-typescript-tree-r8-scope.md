# 0388 — R8.1–R8.22 splits three ways — twelve already closed, five closing as a byproduct of this track's own conversion, two needing named separate slices, one shared with phase 8

- **Status:** Accepted (v0.249.37)

**Context.** The trajectory names both R7.1–R7.8 and R8.1–R8.22 as phase 7's reference rules, but R8 is
chiefly emission-*semantics* — much of which plausibly moved once phase 6's `IrItem`/`Callee`/
`CommitShape` existed as real types, rather than waiting for this phase's tree/printer split. Treating all
21 rules (R8.2–R8.22) as open by inheritance risked this track claiming, and being measured against, work
it doesn't actually need to do. A rule-by-rule audit against the current tree — not the July review's
stale findings — found a three-way split, not the draft's assumed closed/open binary: **twelve rules read
CLOSED** (R8.5, R8.7, R8.9, R8.11, R8.12, R8.13, R8.15, R8.17, R8.18, R8.19, R8.21, R8.22), each with
direct file:line evidence in the current tree; **five read PARTIAL** — behaviourally correct but
structurally sourced from the AST or ad-hoc collection rather than the IR (R8.3's `is_opaque` branches at
five emission call sites instead of reading a pre-decided shape; R8.6's `CommitShape` exists exactly as
specified in `ir.rs` but has zero consumers, with emission still re-deriving the same distinction
independently; R8.8's invariant/transition ordering is exact but iterates the raw `AgentDecl`; R8.10's key
mangling is a single pure function but lacks the rule's own required stated inverse; R8.16's per-consumer
surface generation is already correct but built from an untyped `HashMap`, not a `ProjectGraph`) — each of
these is naturally finished by this track's own tree/printer conversion, needing no separate construction;
**two read genuinely OPEN**, separately scoped (R8.2's brand prefix is computed at emission rather than
read from a recorded brand; R8.14's codec collector still walks raw AST, with its own doc comment
recording that an IR-based conversion was investigated and declined at P6.56 — worth revisiting once a
tree exists to collect over instead). R8.20 was found to be the identical defect to R7.6, already in this
track's scope, not separate work. R8.16's PARTIAL finding splits down the middle with phase 8, which
already owns a typed `ProjectGraph` by name per phase 4's own retirement note.

**Decision.** This track's R8 scope is: the two OPEN rules (R8.2, R8.14) as named Arc D slices; the five
PARTIAL rules close automatically as Arc B/C's own conversion lands, verified rather than separately
built; the twelve CLOSED rules get a single verify-only pass at retirement, with R8.12 flagged explicitly
as self-superseding the moment R7.1's `Any` elimination lands (closed today under its own current text,
but the rule's meaning changes once `Any` is gone — a point worth a reviewer's explicit attention, not a
silent transition that could read as regression). R8.16's data-model half stays phase 8's.

**Consequences.** This is the most load-bearing of this settling pass's four decisions in terms of scope
control — it prevents the track from silently absorbing work several of these rules' own evidence shows
is either already done or genuinely belongs to a different phase. P7.17 (§6 of the track doc) is the
explicit slice this verify-only pass and the R8.12 self-supersession note both ride on.
