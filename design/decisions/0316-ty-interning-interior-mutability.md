# 0316 — The intern table is shared by `&self`, not threaded as `&mut`

- **Status:** Accepted (v0.247.7)

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
