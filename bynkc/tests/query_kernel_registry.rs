//! #596: the enumerable `Query` kernel-method registry and the `store Map`
//! `.entries`/`.keys`/`.values` accessor registry must not drift from the
//! checker's dispatch. Every entry either registry lists is driven through the
//! real checker on a `Query`-typed receiver (a `store Map`'s bare-value
//! reading); none may be rejected as `bynk.types.method_not_found` (a phantom
//! kernel method) or `bynk.store.unknown_map_accessor`/
//! `bynk.types.unknown_field` (a phantom accessor). Args are omitted for the
//! methods, so a recognised one surfaces only as an arity/argument error —
//! fine; we assert solely on the "not found" categories.
//!
//! **The tooth bites one way only**, same limits as `kernel_registry.rs`/
//! `store_op_registry.rs`: this catches a registry entry the checker rejects;
//! it cannot catch the converse (a dispatch arm added later that the table
//! under-lists), and it does not check the signature *strings*.
//!
//! `Query` only ever appears through a `store` field (or a value derived from
//! one), which needs a `context`, so the probe is compiled as a one-file
//! project in a temp directory — the same shape `store_op_registry.rs` uses.

use std::fs;

use bynkc::kernel_methods::{KernelMethod, QUERY_METHODS};
use bynkc::store_ops::MAP_QUERY_ACCESSORS;
use bynkc::{CompileOptions, ProjectFailure, compile_project};

/// Compile a probe agent with `store items: Map[String, Int]` whose handler
/// evaluates each given expression (a `<recv>.<member>` access), and return
/// the diagnostic category codes.
fn probe_codes(tag: &str, exprs: &[String]) -> Vec<String> {
    let mut src = String::from(
        "context probe\n\nagent Probe {\n  key id: String\n  store items: Map[String, Int]\n\n  on call f() -> Effect[()] {\n",
    );
    for e in exprs {
        src += &format!("    let _ = {e}\n");
    }
    src += "    Effect.pure(())\n  }\n}\n";

    let root =
        std::env::temp_dir().join(format!("bynk_query_registry_{}_{tag}", std::process::id()));
    let dir = root.join("src");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("probe.bynk"), &src).unwrap();
    let out = match compile_project(&CompileOptions::single(dir)) {
        Ok(_) => Vec::new(),
        Err(f) => ProjectFailure::flatten(f)
            .iter()
            .map(|e| e.category.to_string())
            .collect(),
    };
    let _ = fs::remove_dir_all(&root);
    out
}

fn method_calls(methods: &[KernelMethod]) -> Vec<String> {
    methods
        .iter()
        .map(|m| format!("items.values.{}()", m.name))
        .collect()
}

#[test]
fn query_kernel_registry_pins_dispatch() {
    let codes = probe_codes("pins", &method_calls(QUERY_METHODS));
    assert!(
        !codes.iter().any(|c| c == "bynk.types.method_not_found"),
        "registry lists method(s) the checker rejects as not-found:\n{}",
        codes.join("\n")
    );
}

/// Negative control: without it, a probe that never reached `Query` dispatch
/// at all (a parse error, an unregistered store field) would report no
/// `method_not_found` and pass vacuously.
#[test]
fn the_probe_actually_reaches_query_dispatch() {
    let codes = probe_codes(
        "control",
        &["items.values.definitelyNotAMethod()".to_string()],
    );
    assert!(
        codes.iter().any(|c| c == "bynk.types.method_not_found"),
        "`items.values.definitelyNotAMethod()` should be an unknown `Query` method, got:\n{}",
        codes.join("\n")
    );
}

#[test]
fn map_query_accessor_registry_pins_dispatch() {
    let exprs: Vec<String> = MAP_QUERY_ACCESSORS
        .iter()
        .map(|a| format!("items.{}", a.name))
        .collect();
    let codes = probe_codes("accessors_pin", &exprs);
    assert!(
        !codes.iter().any(|c| c == "bynk.store.unknown_map_accessor"),
        "registry lists accessor(s) the checker rejects as unknown:\n{}",
        codes.join("\n")
    );
}

/// Negative control for the accessor registry, mirroring the method one above.
#[test]
fn the_probe_actually_reaches_map_accessor_dispatch() {
    let codes = probe_codes(
        "accessors_control",
        &["items.definitelyNotAnAccessor".to_string()],
    );
    assert!(
        codes.iter().any(|c| c == "bynk.store.unknown_map_accessor"),
        "`items.definitelyNotAnAccessor` should be an unknown map accessor, got:\n{}",
        codes.join("\n")
    );
}
