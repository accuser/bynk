# 0318 — The intern table is owned per build, not per `check_record` invocation

- **Status:** Accepted (v0.247.7)

**Context.** The settling review resolved table ownership as "owned per `check_record`
invocation, carried on `TypedCommons`/`CheckedProgram`"
([#1070](https://github.com/accuser/bynk/issues/1070)). That is correct for a single compilation
unit, and it is what the single-unit path does.

It is wrong for the project path. `check_unit_files` runs `check_record` once per unit and funnels
every unit's `expr_types` into one `ExprTypeSink`. If each invocation minted its own table, the
`TyId`s arriving at that sink would come from several tables at once and be mutually ambiguous —
the same `u32` denoting different types depending on which unit produced it.

**Decision.** `checker::check_record_in` takes a caller-supplied `Arc<Types>`. The analysis and
compile entry points mint exactly one table per *build* and carry it on `ProjectAnalysis`,
`InMemoryAnalysis`, `ProjectDiagnostics`, and the LSP's `Analysis`. `check_record` remains, minting
one table for the single-unit case.

**Consequences.** This is strictly safer than the per-unit case #1070 argued for, not a retreat
from it: ids remain comparable only within one table, and on the project path there is now one
table rather than one per unit.

The invariant is "an id is resolved only against the table that minted it", and it is enforced at
runtime rather than in the type system. `Types::get` fails by name — naming the id, the table, and
what to check — instead of surfacing as a bare index-out-of-bounds several frames below the real
mistake. In debug builds the check is identity, not length: each `Types` carries a distinct tag and
each `TyId` carries its table's, so a foreign id is caught even when its index is *in range*. That
is the shape a bounds check cannot see and the one that would otherwise resolve to an unrelated
`Ty` in silence; release builds keep the bounds check, which indexing would have cost anyway.

A static guard would mean a lifetime-branded `TyId<'a>`, infecting every signature and struct field
that names one, across four crates. That was considered and rejected as disproportionate for
compiler-internal wiring that every test run exercises — a judgement the migration's own history
supports: the two wrong-table mistakes made while building T3.6b were both caught by the existing
test suite, and both were the shape the runtime guard names precisely.
