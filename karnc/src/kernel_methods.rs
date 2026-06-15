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
//! The signatures are human-readable Karn-surface display strings (generic in
//! the element/key/value type), for completion `detail` — not the checker's
//! `Ty`-typed signatures. A drift test (`kernel_registry_pins_dispatch`)
//! drives every listed method through the real checker and asserts none is
//! rejected as `method_not_found`, so the table can't list a phantom method.

use crate::ast::BaseType;
use crate::checker::Ty;

/// One built-in kernel method: its name and a display signature.
#[derive(Debug, Clone, Copy)]
pub struct KernelMethod {
    pub name: &'static str,
    pub signature: &'static str,
}

const fn m(name: &'static str, signature: &'static str) -> KernelMethod {
    KernelMethod { name, signature }
}

/// `List[T]` (v0.20b).
pub const LIST_METHODS: &[KernelMethod] = &[
    m("length", "length() -> Int"),
    m("get", "get(index: Int) -> Option[T]"),
    m("prepend", "prepend(item: T) -> List[T]"),
    m("fold", "fold(init: U, step: (U, T) -> U) -> U"),
    m(
        "foldEff",
        "foldEff(init: U, step: (U, T) -> Effect[U]) -> Effect[U]",
    ),
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

/// The value methods of a receiver type, or `&[]` for a type with no kernel
/// methods (record/sum named types, `Bool`, `Effect`, …). Record *fields* are
/// resolved separately by the LSP (they need the type declaration).
pub fn methods_for(ty: &Ty) -> &'static [KernelMethod] {
    match ty {
        Ty::Base(BaseType::Int) => INT_METHODS,
        Ty::Base(BaseType::Float) => FLOAT_METHODS,
        Ty::Base(BaseType::String) => STRING_METHODS,
        Ty::List(_) => LIST_METHODS,
        Ty::Map(_, _) => MAP_METHODS,
        Ty::Option(_) => OPTION_METHODS,
        Ty::Result(_, _) => RESULT_METHODS,
        _ => &[],
    }
}
