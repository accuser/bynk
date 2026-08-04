---
level: patch
changelog: The checker interns every `Ty` behind a `TyId` handle, making type identity a `u32` comparison rather than a recursive structural walk
---

## ADR: ty-interning-interior-mutability
title: The intern table is shared by `&self`, not threaded as `&mut`
summary: Why `Types::intern` takes `&self`, so a `&Types` stays `Copy` alongside the checker's `&mut` state

**Context.** T3.6b (spine [#1046](https://github.com/accuser/bynk/issues/1046), issue
[#1072](https://github.com/accuser/bynk/issues/1072), rules R4.1/R4.2) makes `TyId` — a `Copy`
handle above an intern table — the currency the checker passes around, in place of the recursive
`Ty` values it cloned before. Roughly 200 sites mint a `TyId`, and essentially all of them do it
from inside `Ctx`.

The signature of `intern` is not a free choice, because of where it is called from. `Ctx`'s other
fields — `expr_types`, `errors`, and the check sinks — are `&mut` and are routinely live across an
interning call. `ctx.tys.intern(…)` inside a loop over `ctx.scopes`, with `ctx.errors.push(…)` in
the same block, is the common shape rather than the exception.

**Decision.** `Types` uses interior mutability: `intern(&self, ty: Ty) -> TyId`, with a `Mutex`
inside. A `&Types` is therefore `Copy` and can be read once and reused, independent of whatever
else in `Ctx` is mutably borrowed.

**Consequences.** The alternative — `intern(&mut self)` — would have made the borrow checker,
rather than the type system, the thing all ~200 minting sites were written around: each would need
the table's `&mut` borrow to end before touching `ctx.errors`, which in a loop means either
restructuring the loop or buffering the errors. That is a large, purely incidental cost paid at
every site, to express a table that is conceptually append-only and never aliased for writing.

The price is that `intern` and `get` are not statically prevented from being called re-entrantly
while the lock is held. They are not, today: neither calls back into user code, and both hold the
guard only across a `Vec` push and a `HashMap` insert or lookup. The lock is recovered from
poisoning rather than propagating a panic, because `intern` cannot itself panic while holding it —
a poisoned lock can only mean an unrelated unwind passed a live guard, leaving the table
structurally sound.

## ADR: ty-interning-atomic-not-cell
title: The intern table is `Arc`/`Mutex`, not `Rc`/`RefCell`
summary: The table's threading requirement comes from its LSP consumer, not from the compiler

**Context.** The compiler itself is single-threaded. `bynk-check` and `bynk-emit` would both be
served by an `Rc<RefCell<…>>`, which is cheaper: no atomics on refcount traffic, no lock.

But the table does not stay in the compiler. It rides out on `TypedCommons` and `ProjectAnalysis`
into `bynk-lsp`, whose `tower-lsp` handlers are `async` and therefore require `Send`. A non-atomic
refcount is precisely what `Send` forbids.

**Decision.** `Arc<Types>` with a `Mutex` inside, not `Rc<RefCell<Types>>`.

**Consequences.** The cost is borne by the compiler for a requirement the compiler does not have —
the choice is made by the consumer. It is the right trade anyway: the alternative is two table
types, or a generic parameter threaded through every signature that names one, to avoid an
uncontended lock and an atomic increment.

Contention is not the concern, since there is exactly one writer and every current caller is
single-threaded; per-access cost is. Every `compatible`, `unify`, `substitute` and `contains_var`
recursion level now pays a lock acquire and an `Arc` refcount bump where it previously paid a
pointer dereference, and those walks are frequent enough that the two places interning makes
asymptotically cheaper (`unify`'s prior-binding check and `Ty::Map`'s key equality, both formerly
recursive structural walks, now `u32` equality) are partly funded out of that tax. The table is
append-only and its nodes are immutable, so an `RwLock` would let concurrent readers stop
serialising against each other; that is the first thing to reach for if corpus timings ever say
this matters. They do not today.

## ADR: ty-interning-one-table-per-build
title: The intern table is owned per build, not per `check_record` invocation
summary: A refinement to #1070's settled answer, forced by the project path funnelling every unit into one sink

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
