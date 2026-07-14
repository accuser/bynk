//! #611: the enumerable storage-operation registry.
//!
//! The entry operations of the `store` kinds (`Cell`/`Map`/`Set`/`Cache`/
//! `Log`) are dispatched by the checker in [`crate::checker`]'s
//! `check_store_*_op` functions, where the operation names live in `match`
//! arms — authoritative for *typing*, but not enumerable. This module is the
//! enumerable view the LSP reads for hover on a store operation, mirroring
//! [`crate::kernel_methods`]'s relationship to the value-kernel dispatch.
//!
//! The signatures are human-readable Bynk-surface display strings, generic in
//! the kind's element/key/value type (`K`/`V`/`T` — the store field's declared
//! kind grounds them at the hover site).
//!
//! **What the drift test pins, and what it does not.**
//! `store_op_registry_pins_dispatch` drives every listed operation through the
//! real checker on a `store` field of the matching kind and asserts none is
//! rejected as `unknown_op` — so the table cannot list a **phantom** operation.
//! It does not bite the other way: an operation added to a `check_store_*_op`
//! arm later fails nothing here, and this table will silently **under-list** it
//! (hover then falls through — a missing hover, not a wrong one). Nor does it
//! check the signature **strings**, which are display-only and unread by the
//! checker; those are pinned by eye against the `check_store_*_op` arms.
//! [`crate::kernel_methods`] has the same shape and the same two limits.

/// One storage operation: its name and a display signature.
#[derive(Debug, Clone, Copy)]
pub struct StoreOp {
    pub name: &'static str,
    pub signature: &'static str,
}

const fn op(name: &'static str, signature: &'static str) -> StoreOp {
    StoreOp { name, signature }
}

/// `store Map[K, V]` (v0.82, ADR 0110) — entry-level and effectful.
pub const MAP_STORE_OPS: &[StoreOp] = &[
    op("put", "put(key: K, value: V) -> Effect[()]"),
    op("get", "get(key: K) -> Effect[Option[V]]"),
    op("remove", "remove(key: K) -> Effect[()]"),
    op("contains", "contains(key: K) -> Effect[Bool]"),
    op("size", "size() -> Effect[Int]"),
    op("update", "update(key: K, f: (V) -> V) -> Effect[()]"),
    op(
        "upsert",
        "upsert(key: K, initial: V, f: (V) -> V) -> Effect[()]",
    ),
];

/// `store Cache[K, V]` (v0.87, ADR 0113) — the storage `Map`'s operation set,
/// but its own table rather than an alias: every operation **but `remove`**
/// applies TTL expiry, which reads the clock, so the handler must declare
/// `given Clock`. That requirement is part of the operation's contract, so it is
/// rendered in the signature (as a handler's own `given` clause is written,
/// after the return type) — an alias to [`MAP_STORE_OPS`] would silently drop it.
pub const CACHE_STORE_OPS: &[StoreOp] = &[
    op("put", "put(key: K, value: V) -> Effect[()] given Clock"),
    op("get", "get(key: K) -> Effect[Option[V]] given Clock"),
    // The one op that does not apply expiry, and so does not read the clock.
    op("remove", "remove(key: K) -> Effect[()]"),
    op("contains", "contains(key: K) -> Effect[Bool] given Clock"),
    op("size", "size() -> Effect[Int] given Clock"),
    op(
        "update",
        "update(key: K, f: (V) -> V) -> Effect[()] given Clock",
    ),
    op(
        "upsert",
        "upsert(key: K, initial: V, f: (V) -> V) -> Effect[()] given Clock",
    ),
];

/// `store Set[T]` (v0.83) — entry-level and effectful. Set algebra
/// (`union`/`intersection`/`difference`) is deferred.
pub const SET_STORE_OPS: &[StoreOp] = &[
    op("add", "add(item: T) -> Effect[()]"),
    op("remove", "remove(item: T) -> Effect[()]"),
    op("contains", "contains(item: T) -> Effect[Bool]"),
    op("size", "size() -> Effect[Int]"),
];

/// `store Cell[T]` (v0.98, ADR 0125) — `update` is the only method-shaped
/// operation; a cell is read by its bare name and written with `:=`.
pub const CELL_STORE_OPS: &[StoreOp] = &[op("update", "update(f: (T) -> T) -> Effect[()]")];

/// `store Log[T]` (v0.95, ADR 0121) — `append` is the one effectful write; the
/// time-window roots are lazy `Query[T]` builders. The general query
/// vocabulary the roots feed into is the kernel `Query` surface, not a `Log`
/// operation, so it is not listed here.
pub const LOG_STORE_OPS: &[StoreOp] = &[
    op("append", "append(entry: T) -> Effect[()]"),
    op("since", "since(start: Instant) -> Query[T]"),
    op("before", "before(end: Instant) -> Query[T]"),
    op(
        "between",
        "between(start: Instant, end: Instant) -> Query[T]",
    ),
    op("recent", "recent(count: Int) -> Query[T]"),
    op("reversed", "reversed() -> Query[T]"),
];

/// The operations of the storage kind named `head` (a `StoreKind`'s head
/// identifier — `"Map"`, `"Cell"`, …). `Queue` is in the storage-kind
/// catalogue but has no dispatched operations yet, so it — like an unknown
/// head — yields none.
pub fn ops_for(head: &str) -> &'static [StoreOp] {
    match head {
        "Map" => MAP_STORE_OPS,
        "Cache" => CACHE_STORE_OPS,
        "Set" => SET_STORE_OPS,
        "Cell" => CELL_STORE_OPS,
        "Log" => LOG_STORE_OPS,
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ops_for_maps_the_catalogue_and_ignores_unknown_heads() {
        // Assert a *reachable op* per kind, not a table property — `ops_for`
        // mapping a head to the wrong table is exactly what this must catch.
        assert!(ops_for("Map").iter().any(|o| o.name == "put"));
        assert!(ops_for("Cache").iter().any(|o| o.name == "put"));
        assert!(ops_for("Set").iter().any(|o| o.name == "add"));
        assert!(ops_for("Cell").iter().all(|o| o.name == "update"));
        assert!(ops_for("Log").iter().any(|o| o.name == "append"));
        // A `Set` has no `put` and a `Map` no `add` — the tables are distinct.
        assert!(!ops_for("Set").iter().any(|o| o.name == "put"));
        assert!(!ops_for("Map").iter().any(|o| o.name == "add"));
        // `Queue` is a catalogue kind with no dispatched ops; `Nope` is unknown.
        assert!(ops_for("Queue").is_empty());
        assert!(ops_for("Nope").is_empty());
    }

    /// A `Cache` op reads the clock for TTL expiry — every one but `remove` — so
    /// its signature says so. This is the table's reason for existing separately
    /// from [`MAP_STORE_OPS`]; an alias would drop the requirement silently.
    #[test]
    fn cache_signatures_carry_the_clock_requirement_except_remove() {
        for o in CACHE_STORE_OPS {
            let wants_clock = o.name != "remove";
            assert_eq!(
                o.signature.contains("given Clock"),
                wants_clock,
                "`Cache.{}` clock requirement is misrendered: {:?}",
                o.name,
                o.signature
            );
        }
        // The op *names* still mirror the storage `Map`'s set exactly.
        let names = |ops: &[StoreOp]| ops.iter().map(|o| o.name).collect::<Vec<_>>();
        assert_eq!(names(CACHE_STORE_OPS), names(MAP_STORE_OPS));
        // …and the storage `Map` itself requires no clock.
        assert!(!MAP_STORE_OPS.iter().any(|o| o.signature.contains("given")));
    }

    #[test]
    fn signatures_lead_with_their_operation_name() {
        for ops in [
            MAP_STORE_OPS,
            CACHE_STORE_OPS,
            SET_STORE_OPS,
            CELL_STORE_OPS,
            LOG_STORE_OPS,
        ] {
            assert!(!ops.is_empty());
            for o in ops {
                assert!(!o.name.is_empty());
                assert!(
                    o.signature.starts_with(o.name),
                    "signature {:?} should lead with {:?}",
                    o.signature,
                    o.name
                );
            }
        }
    }
}
