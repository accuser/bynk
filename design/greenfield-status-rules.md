<!-- GENERATED, APPEND-ONLY — written by `cargo xtask stamp --apply` (xtask/src/stamp.rs) at merge, one row per rule id an increment cites in its `closes_rule` frontmatter (#1001).
     Read by `cargo xtask greenfield-status` to populate the reference's rule-citation surface. Do not hand-edit. -->

# Rules closed

| Rule | Version | PR | Changelog |
|---|---|---|---|
| R10.4 | v0.247.10 |  | **Breaking (Rust API, not language surface):** `bynkc` no longer re-exports `bynk-syntax`'s `ast`/`diagnostics`/`error`/`keywords`/`lexer`/`parser`/`span` modules, `bynk-driver`'s `coverage`/`test_json` modules, or the whole `bynk-fmt` crate as `bynkc::fmt` (only `bynkc`'s item re-exports, `CompileError`, `CompileOptions`, `compile_project`, and others, remain public) |
| R6.2 | v0.247.1 |  | The lowering pass returns hoisted statements instead of writing them into a caller-supplied sink, deleting the predictive classifier that gated the ternary-form `if` |
| R11.6 | v0.246.8 |  | The Query method_not_found diagnostic lists its methods from QUERY_METHODS instead of a hand-written copy, and the registry's drift test now catches a dispatch arm the registry doesn't list, not only the reverse |
| R6.11 | v0.246.8 |  | The Query method_not_found diagnostic lists its methods from QUERY_METHODS instead of a hand-written copy, and the registry's drift test now catches a dispatch arm the registry doesn't list, not only the reverse |
