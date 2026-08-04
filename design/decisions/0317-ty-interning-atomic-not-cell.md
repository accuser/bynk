# 0317 — The intern table is `Arc`/`Mutex`, not `Rc`/`RefCell`

- **Status:** Accepted (v0.247.7)

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
