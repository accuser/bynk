//! Storage-track `store`/`Cell` checking: valid `Cell` usage compiles cleanly,
//! and each misuse raises its specific diagnostic.
//!
//! Agents need a `context`, which the single-file `compile` API does not accept,
//! so each case is compiled as a one-file project in a temp directory.

use std::fs;

use bynkc::{CompileOptions, ProjectFailure, compile_project};

/// Compile `bynk` as `context shop` (one-file project) and return its diagnostic
/// category codes (empty on success).
fn codes(tag: &str, bynk: &str) -> Vec<String> {
    let root = std::env::temp_dir().join(format!("bynk_store_{}_{tag}", std::process::id()));
    let src = root.join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("shop.bynk"), bynk).unwrap();
    let out = match compile_project(&CompileOptions::single(src.clone())) {
        Ok(_) => Vec::new(),
        Err(f) => ProjectFailure::flatten(f)
            .iter()
            .map(|e| e.category.to_string())
            .collect(),
    };
    let _ = fs::remove_dir_all(&root);
    out
}

/// A minimal agent in `context shop` whose handler body is `body`, around the
/// store `field`. Used by the misuse tests.
fn agent_with(field: &str, body: &str) -> String {
    format!(
        "context shop\n\nagent A {{\n  key id: String\n  {field}\n  \
         on call f(p: Int) -> Effect[()] {{ {body}\n    Effect.pure(()) }}\n}}\n"
    )
}

#[test]
fn valid_cell_agent_compiles_cleanly() {
    // Bare `Cell` reads (`count`, the `count >= 0` invariant), two `:=` writes,
    // and a read handler — all valid. No diagnostics.
    let src = "context shop\n\
        \n\
        agent Counter {\n\
        \x20 key id: String\n\
        \x20 store count: Cell[Int] = 0\n\
        \x20 store flag: Cell[Bool] = false\n\
        \n\
        \x20 invariant nonneg: count >= 0\n\
        \n\
        \x20 on call setTo(n: Int) -> Effect[()] {\n\
        \x20   count := n\n\
        \x20   flag := true\n\
        \x20   Effect.pure(())\n\
        \x20 }\n\
        \x20 on call read() -> Effect[Int] {\n\
        \x20   count\n\
        \x20 }\n\
        }\n";
    assert_eq!(
        codes("valid", src),
        Vec::<String>::new(),
        "a valid Cell agent must compile with no diagnostics"
    );
}

#[test]
fn assign_self_reference_is_rejected() {
    let cs = codes(
        "selfref",
        &agent_with("store count: Cell[Int] = 0", "count := count + 1"),
    );
    assert!(
        cs.contains(&"bynk.cell.self_reference".to_string()),
        "{cs:?}"
    );
}

#[test]
fn assign_type_mismatch_is_rejected() {
    let cs = codes(
        "mismatch",
        &agent_with("store count: Cell[Int] = 0", "count := \"x\""),
    );
    assert!(
        cs.contains(&"bynk.types.type_mismatch".to_string()),
        "{cs:?}"
    );
}

#[test]
fn assign_to_non_cell_target_is_rejected() {
    // `p` is a handler parameter, not a store cell.
    let cs = codes(
        "nontarget",
        &agent_with("store count: Cell[Int] = 0", "p := 1"),
    );
    assert!(
        cs.contains(&"bynk.cell.invalid_target".to_string()),
        "{cs:?}"
    );
}

#[test]
fn unsupported_kind_is_gated() {
    // `Cache` is a known kind but not yet functional.
    let cs = codes("cache", &agent_with("store c: Cache[String, Int]", ""));
    assert!(
        cs.contains(&"bynk.store.kind_unsupported".to_string()),
        "{cs:?}"
    );
}

#[test]
fn unknown_kind_is_rejected() {
    let cs = codes("widget", &agent_with("store w: Widget[Int]", ""));
    assert!(
        cs.contains(&"bynk.store.unknown_kind".to_string()),
        "{cs:?}"
    );
}

#[test]
fn cell_arity_is_checked() {
    let cs = codes("arity", &agent_with("store c: Cell[Int, Int] = 0", ""));
    assert!(cs.contains(&"bynk.store.kind_arity".to_string()), "{cs:?}");
}

// -- v0.83 storage Map (ADR 0110) --

#[test]
fn valid_map_agent_compiles_cleanly() {
    let cs = codes(
        "mapok",
        &agent_with("store m: Map[String, Int]", "let _ <- m.put(\"k\", p)"),
    );
    assert_eq!(
        cs,
        Vec::<String>::new(),
        "a valid Map agent must compile clean: {cs:?}"
    );
}

#[test]
fn map_unknown_op_is_rejected() {
    let cs = codes(
        "mapop",
        &agent_with("store m: Map[String, Int]", "let _ <- m.frobnicate(\"k\")"),
    );
    assert!(cs.contains(&"bynk.store.unknown_op".to_string()), "{cs:?}");
}

#[test]
fn map_op_arg_type_is_checked() {
    // The key type is `String`; passing an `Int` key is a mismatch.
    let cs = codes(
        "mapkey",
        &agent_with("store m: Map[String, Int]", "let _ <- m.put(5, 5)"),
    );
    assert!(
        cs.contains(&"bynk.types.argument_mismatch".to_string()),
        "{cs:?}"
    );
}

// -- v0.84 storage Set (ADR 0110) --

#[test]
fn valid_set_agent_compiles_cleanly() {
    let cs = codes(
        "setok",
        &agent_with("store s: Set[Int]", "let _ <- s.add(p)"),
    );
    assert_eq!(
        cs,
        Vec::<String>::new(),
        "a valid Set agent must compile clean: {cs:?}"
    );
}

#[test]
fn set_unknown_op_is_rejected() {
    let cs = codes(
        "setop",
        &agent_with("store s: Set[Int]", "let _ <- s.frobnicate(p)"),
    );
    assert!(cs.contains(&"bynk.store.unknown_op".to_string()), "{cs:?}");
}

#[test]
fn set_op_arg_type_is_checked() {
    // The element type is `Int`; adding a `String` is a mismatch.
    let cs = codes(
        "setelem",
        &agent_with("store s: Set[Int]", "let _ <- s.add(\"x\")"),
    );
    assert!(
        cs.contains(&"bynk.types.argument_mismatch".to_string()),
        "{cs:?}"
    );
}

// -- v0.85 storage annotations (ADR 0111) --

#[test]
fn annotation_indexed_on_map_parses_and_gates() {
    // `@indexed` is the right annotation on a `Map`, so it parses and resolves —
    // but no annotation is functional yet (slice 3a is grammar + registry only),
    // so it gates as unsupported (not a parse error, not unknown, not mismatch).
    let cs = codes(
        "annidx",
        &agent_with("store m: Map[String, Int] @indexed(by: id)", ""),
    );
    assert!(
        cs.contains(&"bynk.store.annotation_unsupported".to_string()),
        "{cs:?}"
    );
    assert!(
        !cs.contains(&"bynk.store.unknown_annotation".to_string())
            && !cs.contains(&"bynk.store.annotation_kind_mismatch".to_string()),
        "a well-formed `@indexed` on a Map must only gate as unsupported: {cs:?}"
    );
}

#[test]
fn annotation_unknown_name_is_rejected() {
    let cs = codes(
        "annunk",
        &agent_with("store m: Map[String, Int] @frobnicate(1)", ""),
    );
    assert!(
        cs.contains(&"bynk.store.unknown_annotation".to_string()),
        "{cs:?}"
    );
}

#[test]
fn annotation_on_wrong_kind_is_rejected() {
    // `@ttl` belongs on `Cache`, not `Map` — a kind mismatch, and the mismatch
    // wins over the unsupported gate (we stop at the first failure).
    let cs = codes(
        "annkind",
        &agent_with("store m: Map[String, Int] @ttl(5.minutes)", ""),
    );
    assert!(
        cs.contains(&"bynk.store.annotation_kind_mismatch".to_string()),
        "{cs:?}"
    );
    assert!(
        !cs.contains(&"bynk.store.annotation_unsupported".to_string()),
        "kind mismatch must short-circuit the unsupported gate: {cs:?}"
    );
}

#[test]
fn annotation_duration_literal_arg_parses() {
    // `5.minutes` is parsed as an ordinary expression argument (the `Duration`
    // literal lands in slice 3b); the grammar must accept it without a parse
    // error — the field gates on `@ttl`/`Cache` being unsupported, nothing more.
    let cs = codes(
        "anndur",
        &agent_with("store c: Cache[String, Int] @ttl(5.minutes)", ""),
    );
    assert!(
        !cs.iter().any(|c| c.starts_with("bynk.parse")),
        "the duration-literal argument must parse: {cs:?}"
    );
    assert!(
        cs.contains(&"bynk.store.annotation_unsupported".to_string()),
        "{cs:?}"
    );
}
