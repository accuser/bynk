//! v0.30.2 (ADR 0063): the enumerable kernel-method registry.
//!
//! The value methods of the built-in kernels (`List`/`Map`/`Option`/`Result`/
//! `String`/`Int`/`Float`) are dispatched by the checker in
//! [`crate::checker`]'s `check_*_kernel_method` functions, where the method
//! names live in `match` arms — authoritative for *typing*, but not
//! enumerable. This module is the enumerable view the LSP reads for `.`-member
//! completion: per-kernel `(name, signature)` tables and a [`methods_for`]
//! mapping from a receiver [`Ty`] to its methods.
//!
//! The signatures are human-readable Bynk-surface display strings (generic in
//! the element/key/value type), for completion `detail` — not the checker's
//! `Ty`-typed signatures. A drift test (`kernel_registry_pins_dispatch`)
//! drives every listed method through the real checker and asserts none is
//! rejected as `method_not_found`, so the table can't list a phantom method.

use crate::checker::{NamedKind, Ty};
use bynk_syntax::ast::BaseType;

/// One built-in kernel method: its name and a display signature.
#[derive(Debug, Clone, Copy)]
pub struct KernelMethod {
    pub name: &'static str,
    pub signature: &'static str,
}

const fn m(name: &'static str, signature: &'static str) -> KernelMethod {
    KernelMethod { name, signature }
}

/// `List[T]` (v0.20b; v0.88 adds the ADR 0116 query/collection vocabulary).
pub const LIST_METHODS: &[KernelMethod] = &[
    m("length", "length() -> Int"),
    m("get", "get(index: Int) -> Option[T]"),
    m("prepend", "prepend(item: T) -> List[T]"),
    m("fold", "fold(init: U, step: (U, T) -> U) -> U"),
    m(
        "foldEff",
        "foldEff(init: U, step: (U, T) -> Effect[U]) -> Effect[U]",
    ),
    // v0.146 (ADR 0170): run an effectful step for each element, in order.
    m("forEach", "forEach(f: T -> Effect[()]) -> Effect[()]"),
    // v0.88 (ADR 0116): eager in-memory builders + terminals.
    m("map", "map(f: T -> U) -> List[U]"),
    m("filter", "filter(p: T -> Bool) -> List[T]"),
    m("flatMap", "flatMap(f: T -> List[U]) -> List[U]"),
    m("sortBy", "sortBy(key: T -> K) -> List[T]"),
    m("take", "take(n: Int) -> List[T]"),
    m("skip", "skip(n: Int) -> List[T]"),
    m("distinct", "distinct() -> List[T]"),
    m("distinctBy", "distinctBy(key: T -> K) -> List[T]"),
    m("count", "count() -> Int"),
    m("any", "any(p: T -> Bool) -> Bool"),
    m("all", "all(p: T -> Bool) -> Bool"),
    m("first", "first() -> Option[T]"),
    m("firstOrElse", "firstOrElse(default: T) -> T"),
    m("sum", "sum(key: T -> K) -> K"),
    m("min", "min(key: T -> K) -> Option[K]"),
    m("max", "max(key: T -> K) -> Option[K]"),
    m("average", "average(key: T -> K) -> Option[Float]"),
];

/// `Map[K, V]` (v0.20b).
pub const MAP_METHODS: &[KernelMethod] = &[
    m("length", "length() -> Int"),
    m("keys", "keys() -> List[K]"),
    m("get", "get(key: K) -> Option[V]"),
    m("insert", "insert(key: K, value: V) -> Map[K, V]"),
];

/// `Option[T]` combinators (v0.22a).
pub const OPTION_METHODS: &[KernelMethod] = &[
    m("map", "map(f: T -> U) -> Option[U]"),
    m("andThen", "andThen(f: T -> Option[U]) -> Option[U]"),
    m("getOrElse", "getOrElse(default: T) -> T"),
    m("isSome", "isSome() -> Bool"),
    m("okOr", "okOr(err: E) -> Result[T, E]"),
];

/// `Result[T, E]` combinators (v0.22a).
pub const RESULT_METHODS: &[KernelMethod] = &[
    m("map", "map(f: T -> U) -> Result[U, E]"),
    m("andThen", "andThen(f: T -> Result[U, E]) -> Result[U, E]"),
    m("mapErr", "mapErr(f: E -> F) -> Result[T, F]"),
    m("getOrElse", "getOrElse(default: T) -> T"),
    m("isOk", "isOk() -> Bool"),
];

/// The `String` kernel (v0.22a; UTF-16 code units, except `chars`).
pub const STRING_METHODS: &[KernelMethod] = &[
    m("length", "length() -> Int"),
    m("split", "split(sep: String) -> List[String]"),
    m("trim", "trim() -> String"),
    m("toUpper", "toUpper() -> String"),
    m("toLower", "toLower() -> String"),
    m("contains", "contains(s: String) -> Bool"),
    m("startsWith", "startsWith(s: String) -> Bool"),
    m("endsWith", "endsWith(s: String) -> Bool"),
    m("replace", "replace(from: String, to: String) -> String"),
    m("slice", "slice(start: Int, end: Int) -> String"),
    m("indexOf", "indexOf(s: String) -> Option[Int]"),
    m("chars", "chars() -> List[String]"),
    m("concat", "concat(s: String) -> String"),
];

/// The `Int` numeric kernel (v0.21).
pub const INT_METHODS: &[KernelMethod] = &[
    m("toFloat", "toFloat() -> Float"),
    m("toString", "toString() -> String"),
    m("abs", "abs() -> Int"),
    m("min", "min(other: Int) -> Int"),
    m("max", "max(other: Int) -> Int"),
    m("clamp", "clamp(lo: Int, hi: Int) -> Int"),
];

/// The `Float` numeric kernel (v0.21).
pub const FLOAT_METHODS: &[KernelMethod] = &[
    m("round", "round() -> Int"),
    m("floor", "floor() -> Int"),
    m("ceil", "ceil() -> Int"),
    m("truncate", "truncate() -> Int"),
    m("toString", "toString() -> String"),
    m("abs", "abs() -> Float"),
    m("min", "min(other: Float) -> Float"),
    m("max", "max(other: Float) -> Float"),
    m("clamp", "clamp(lo: Float, hi: Float) -> Float"),
    m("isNaN", "isNaN() -> Bool"),
    m("isFinite", "isFinite() -> Bool"),
];

/// The `Duration` kernel (v0.86, ADR 0112). Comparison/arithmetic are operators
/// (D3/D4); the kernel is the explicit escape to raw milliseconds (D5).
pub const DURATION_METHODS: &[KernelMethod] = &[
    m("toMillis", "toMillis() -> Int"),
    m("toString", "toString() -> String"),
];

/// The `Instant` kernel (v0.90, ADR 0114). Comparison/arithmetic are operators
/// (D3); the kernel is the explicit escape to raw epoch milliseconds (D6).
pub const INSTANT_METHODS: &[KernelMethod] = &[
    m("toEpochMillis", "toEpochMillis() -> Int"),
    m("toString", "toString() -> String"),
];

/// The `Bytes` kernel (v0.110, ADR 0142). Equality is an operator (D4, content
/// compare); the kernel is length plus the String-interop bridge (D3). No
/// ordering/arithmetic/concat/slice in v1 (deferred follow-ons).
pub const BYTES_METHODS: &[KernelMethod] = &[
    m("length", "length() -> Int"),
    m("toBase64", "toBase64() -> String"),
    m("decodeUtf8", "decodeUtf8() -> Option[String]"),
];

/// The value methods of a receiver type, or `&[]` for a type with no kernel
/// methods (record/sum named types, `Bool`, `Effect`, …). Record *fields* are
/// resolved separately by the LSP (they need the type declaration).
pub fn methods_for(ty: &Ty) -> &'static [KernelMethod] {
    match ty {
        Ty::Base(BaseType::Int) => INT_METHODS,
        Ty::Base(BaseType::Float) => FLOAT_METHODS,
        Ty::Base(BaseType::Duration) => DURATION_METHODS,
        Ty::Base(BaseType::Instant) => INSTANT_METHODS,
        Ty::Base(BaseType::Bytes) => BYTES_METHODS,
        Ty::Base(BaseType::String) => STRING_METHODS,
        Ty::List(_) => LIST_METHODS,
        Ty::Map(_, _) => MAP_METHODS,
        Ty::Option(_) => OPTION_METHODS,
        Ty::Result(_, _) => RESULT_METHODS,
        // #561: a refined receiver inherits its base type's read-only kernel
        // methods (after its own declared methods, which the completion layer
        // merges in), so `.`-member completion offers them. `Bool` has no
        // kernel; opaque types deliberately do not widen, so both surface none.
        Ty::Named {
            kind: NamedKind::Refined(base),
            ..
        } => match base {
            BaseType::Int => INT_METHODS,
            BaseType::Float => FLOAT_METHODS,
            BaseType::Duration => DURATION_METHODS,
            BaseType::Instant => INSTANT_METHODS,
            BaseType::Bytes => BYTES_METHODS,
            BaseType::String => STRING_METHODS,
            BaseType::Bool => &[],
        },
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refined(base: BaseType) -> Ty {
        Ty::Named {
            name: "R".to_string(),
            kind: NamedKind::Refined(base),
        }
    }

    #[test]
    fn refined_receiver_inherits_base_kernel_methods() {
        // #561: completion on a refined receiver offers the base kernel.
        assert_eq!(
            methods_for(&refined(BaseType::String)).len(),
            STRING_METHODS.len()
        );
        assert_eq!(
            methods_for(&refined(BaseType::Int)).len(),
            INT_METHODS.len()
        );
        assert_eq!(
            methods_for(&refined(BaseType::Float)).len(),
            FLOAT_METHODS.len()
        );
        // `Bool` has no kernel, so a `Bool`-based refinement inherits nothing.
        assert!(methods_for(&refined(BaseType::Bool)).is_empty());
        // Opaque types do not widen — no inherited kernel.
        assert!(
            methods_for(&Ty::Named {
                name: "O".to_string(),
                kind: NamedKind::Opaque(BaseType::String),
            })
            .is_empty()
        );
    }
}
