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
    let cs = codes("map", &agent_with("store m: Map[String, Int]", ""));
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
