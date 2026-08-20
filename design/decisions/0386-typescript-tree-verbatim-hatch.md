# 0386 — The migration escape hatch is a statement-level `Verbatim` node with a closed origin enum and a companion textual lint

- **Status:** Accepted (v0.249.37)

**Context.** Converting ~1,540 TypeScript-producing sites cannot happen in one slice without violating
trajectory §2 ("a phase that half-lands leaves two paths reachable... the failure mode every regret in
this corpus shares"), but converting all of them atomically has no precedent at this scale either — phase
6 needed 59 slices against a smaller surface. This track's own research recovered two prior migration
techniques from this trajectory and found neither transfers directly. Phase 3's "parallel-data" technique
(`identity-and-totality.md`) kept old and new *representations* of the same fact live simultaneously,
safe because every consumer action was the same kind (read a map) regardless of which table backed it —
no behaviour depended on which representation was live at a given moment. Phase 6's IR migration used no
bridge type at all: AST-reading and IR-reading call sites simply coexisted, tracked by a per-file import
count, safe because the *output-producing mechanism* (`writeln!`) never varied regardless of which input
model fed it. Phase 7 differs structurally from both: R7.2–R7.4 exist specifically to make the *writer*
itself singular, so a bare "some sites still call `writeln!` directly" approach — phase 6's own approach
— would be a literal rule violation during this phase, not a benign coexistence. Separately, this track's
research also surfaced a risk the initial framing didn't carry: a byte-golden fixture, this migration's
only cheap correctness check across ~1,540 sites, cannot see *inside* an opaque escape-hatch node. ADR
0198 names the general shape of this failure directly — a defect survived 331 negative fixtures for 60
increments because the fixture format asserted category strings, never the actual attributed value,
"the weakest possible evidence" in that ADR's own words. A `Verbatim` block hiding `enum`/`: any`/
`namespace` would pass every golden fixture unchanged while defeating R7.1's "cannot be typed" claim.

**Decision.** The hatch is `TsStmt::Verbatim { origin: VerbatimOrigin, text: String }`: a sealed
constructor, statement granularity only (not expression-level, which would compose invisibly inside nodes
the tree claims cannot express banned constructs), tagged by a closed `VerbatimOrigin` enum with one
variant per named residue family, so the ratchet is a compile-time construct rather than a grep. The
printer still owns the buffer, indentation and offset arithmetic for a `Verbatim` block from the slice
that introduces it, so R7.3/R7.4 hold throughout the migration, not only once it completes. **A companion
textual lint over `Verbatim` content — forbidding `enum`, `namespace`, decorators, constructor parameter
properties, `: any`/`as any` by pattern match on the wrapped text — ships in the same slice as the hatch
itself (P7.5) and runs in CI alongside the golden fixtures, not as a follow-up.**

**Consequences.** Every Arc C conversion slice has a genuine, individually safe stopping point: before
`Artefacts` (P7.6) lands, stopping anywhere is safe by construction; after it, an unconverted site routes
through `Verbatim`, which the printer already owns per R7.3/R7.4, and the textual lint catches a banned
construct hiding inside one immediately rather than only at full conversion. `verbatim_origins` becomes
a completion probe (§5 of the track doc), retiring at a named, argued floor rather than 0, mirroring
`ast_importers`'s own re-settled floor from phase 6 — but not the *only* new probe: a PR review of this
settling pass found `verbatim_origins` alone gameable, since distinct enum variants don't bound residual
volume (a wholesale, undecomposed wrap of `emitter/emit.rs` or `emitter/lower.rs` into one variant each
would satisfy a small floor while converting nothing). §5 adds `verbatim_sites` — a count of distinct
`Verbatim` construction call sites, retiring at 0 — as the probe that actually tracks conversion progress,
alongside `verbatim_origins`'s tracking of how many residue *families* remain.
