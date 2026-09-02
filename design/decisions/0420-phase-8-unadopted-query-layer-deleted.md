# 0420 — Phase 8's definition- and project-level queries are deleted, not adopted — the unit-level firewall proof stays

- **Status:** Accepted (v0.289.65)

**Context.** Phase 8 (`incrementality.md`, spine #1507) built all four of R3.13's query levels and
retired on 30 August 2026 with `incremental_query_types` reading `4/4` — an existence proof, by its own
settled design (ADR 0414). The same day's post-restructuring review (Part 2) showed what existence
did not: `bynk-project`'s shared parse cache (P8.4) had two production consumers; `UnitSignature`
(P8.1) was read only by its stability test (P8.2); `ProjectGraph` (P8.3, 174 lines) and
`Body(DefId)`/`TypeOf(DefId)` (P8.5, 816 lines) were read by nothing but their own tests.
`queries.rs`'s own first paragraph said so: "nothing in the tree calls `body`/`type_of` yet; a future
scheduler slice (R3.15) is what would wire them in." R3.15's scheduler is deferred with a trigger
(#1523) that cannot fire before a latency measurement exists, and none does. #1537 asked for the same
decision the review asked for phase 6: adopt or delete, written down with a trigger.

Re-verified on `main` at this decision (2 September 2026): `project_graph_for` has one caller, its
fidelity test; `body`/`type_of` have none outside `queries.rs`'s own test module; `unit_signature_for`
is called from the stability test and `unit_signature.rs`'s tests; `canon_unit_signature` (the
contract-hash canonicaliser R3.14's rationale cites) is called only through `UnitSignature::canonical`.
The cycle detection and compose-root generation that a `ProjectGraph` would notionally serve run today
over `project_model.rs`'s own resolved `uses`/`consumes` maps — the exact maps `project_graph_for`
re-packages — so adopting it would relocate a read, not remove one (ADR 0381's own test for a
conversion worth declining).

**Decision.** `bynk-check/src/queries.rs`, `bynk-check/src/project_graph.rs` and
`bynk-check/tests/project_graph_fidelity.rs` are deleted. `bynk-check/src/unit_signature.rs` and
`bynk-check/tests/unit_signature_stability.rs` stay: `UnitSignature` is the R3.14 firewall's own
specification and the artefact #1523's trigger presupposes ("keystroke-to-diagnostic latency …
attributed by level" needs a unit level to attribute to), and its only reader being a proof is the
point, not a defect — it is not a second path competing with anything. `incremental_query_types` is
re-settled to certify this decision in both directions: `UnitSignature` present, the shared cache
migrated, the stability test present, **and** the definition/project levels absent — re-adding
`ProjectGraph` or a `DefId`-keyed `body`/`type_of` changes the committed reading and fails the
currency gate, so neither can return without a consumer and a re-settling.

This supersedes the "landed" status of ADR 0415 (`ProjectGraph`'s shape and placement) and ADR 0417
(`DefId` and the query functions) — both stand as the record of what was built and why, and their
decisions are the ones to rebuild against if R3.15's trigger fires. It does not touch ADR 0412/0413
(`UnitSignature`, the shared cache), R3.13's table (the specification of the destination), R3.14, or
#1523's trigger, which is unchanged.

**Consequences.** `bynk-check` loses 990 lines and one integration test with no production effect —
`cargo test --workspace` and the full e2e corpus are unchanged. Part 15.1's query-framework entry now
states which levels are adopted and which were deleted; the trajectory's closing note records the
outcome beside the IR cutover's. What is lost is named: two working, tested implementations of
R3.13's lower levels, retrievable from history, and each "a few hundred lines" (R3.15's own sizing)
to rebuild against `checker::check_body`/`check_handler_body` when a scheduler is actually wanted. The
30 August review's own Part 5 §5 asked for exactly this choice to be made on the same terms as phase
6's; it now has been, and the last open decision that review raised is closed.
