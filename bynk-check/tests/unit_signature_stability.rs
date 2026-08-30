//! P8.2 (#1513): the R3.14 proof — `UnitSignature` (P8.1, #1512) is stable
//! under an edit to any `Body(DefId)` within that unit. Not a new type, not
//! new checker logic: a property test over machinery P8.1 already built,
//! per `design/tracks/incrementality.md` §3.4 (Q4) and ADR 0412's own
//! closing line ("P8.2 builds against \[P8.1's\] exact field list and its
//! canonical rendering").
//!
//! [DECISION A]: a body edit here means a literal **source-text mutation and
//! re-parse**, not an in-memory AST splice — mirroring how a real edit
//! actually reaches the compiler (a keystroke changes source text, not an
//! AST node) and exercising the same span-shift hazard PR #1509's own bot
//! review turned on (editing a body shifts every later declaration's own
//! spans in the same file). An AST splice could accidentally leave later
//! spans unchanged, masking exactly the defect class this proof exists to
//! catch.
//!
//! [DECISION B]: a companion negative case (a `store`-field addition — a
//! genuine signature change, never a body edit) proves the comparison isn't
//! vacuously satisfied — the same "prove the probe isn't vacuous" discipline
//! P8.0's own `query_types_found_does_not_count_checkers_pre_existing_type_of`
//! regression test already modeled for a different probe.
//!
//! [DECISION C]: coverage edits a body in each of the two shapes that
//! actually carry one per ADR 0412's own field list — a free function and an
//! agent handler. `store` fields have no body to edit (`StoreField.init` is
//! excluded from `UnitSignature` entirely, not compared at all), so a
//! "stability under a store-field edit" case would test something R3.14
//! doesn't claim — used instead, deliberately, as [DECISION B]'s
//! signature-changing negative case.

use std::collections::HashMap;
use std::path::PathBuf;

use bynk_check::symbols::{build_unit_table, combined_types_for};
use bynk_check::unit_signature::unit_signature_for;
use bynk_project::{ParsedFile, UnitKind};
use bynk_syntax::ast::SourceUnit;

fn parsed(source_path: &str, src: &str) -> ParsedFile {
    let tokens = bynk_syntax::lexer::tokenize(src).expect("lex");
    let unit = bynk_syntax::parser::parse_unit(&tokens, src).expect("parse");
    let kind = match &unit {
        SourceUnit::Context(_) => UnitKind::Context,
        SourceUnit::Commons(_) | SourceUnit::Suite(_) | SourceUnit::Adapter(_) => {
            unreachable!("fixture is a context")
        }
    };
    ParsedFile::new(
        PathBuf::from(source_path),
        PathBuf::from(source_path),
        None,
        src.to_string(),
        unit,
        kind,
        false,
    )
}

/// The fixture unit: a free function (`add`) and an agent (`Counter`) whose
/// single handler (`increment`) carries a `body`, plus a `store` field with
/// no body of its own — the shape [DECISION C] needs.
const BASE_SRC: &str = r#"context demo.stability

fn add(a: Int, b: Int) -> Int {
  a + b
}

agent Counter {
  key id: String

  store count: Cell[Int]

  on call increment() -> Effect[Int] {
    count := count + 1
    Effect.pure(count)
  }
}
"#;

/// Builds a canonical `UnitSignature` rendering for `demo.stability` parsed
/// from `src`. No `uses`/commons in this fixture — R3.14 is a claim about one
/// unit's own stability, not about cross-context type resolution (P8.1's own
/// test already exercises that category).
fn canonical_for(src: &str) -> String {
    let file = parsed("demo/stability.bynk", src);
    let mut errors = Vec::new();
    let table = build_unit_table(
        "demo.stability",
        UnitKind::Context,
        &[0],
        &[file],
        &mut errors,
    );
    assert!(errors.is_empty(), "fixture must parse cleanly: {errors:?}");

    let unit_tables = HashMap::from([("demo.stability".to_string(), table.clone())]);
    let unit_uses = HashMap::new();
    let combined = combined_types_for("demo.stability", &unit_tables, &unit_uses);
    unit_signature_for("demo.stability", &table, combined).canonical()
}

/// The R3.14 proof itself ([DECISION A]): editing only a function body and
/// only a handler body — literal source-text mutation, real re-parse — must
/// leave `UnitSignature`'s canonical rendering byte-for-byte unchanged. Both
/// edits land in the same source pass so the second body's own declaration
/// sits *after* the first edit in the file, exercising the byte-offset span
/// cascade a real edit produces (PR #1509's own bot-review finding, one
/// level up in `UnitSignature`'s own design) rather than an isolated,
/// easier-to-get-right single edit.
#[test]
fn unit_signature_is_stable_under_a_body_only_edit() {
    let before = canonical_for(BASE_SRC);

    let edited = BASE_SRC
        .replace("a + b\n}", "let sum = a + b\n  sum\n}")
        .replace(
            "count := count + 1\n    Effect.pure(count)",
            "count := count + 1 + 0\n    Effect.pure(count)",
        );
    assert_ne!(
        edited, BASE_SRC,
        "the fixture edit must actually touch the source (guards against a typo'd replace)"
    );

    let after = canonical_for(&edited);
    assert_eq!(
        before, after,
        "a body-only edit (fn `add` and handler `increment`) must not move UnitSignature's canonical form"
    );
}

/// [DECISION B]: the negative case. Adding a new `store` field is never a
/// body edit — `Counter` gains real, externally-relevant state — so it MUST
/// change the canonical form. Without this case, a comparison bug that
/// always reports "equal" would let the positive test above pass for the
/// wrong reason.
#[test]
fn unit_signature_is_not_stable_under_a_genuine_signature_edit() {
    let before = canonical_for(BASE_SRC);

    let edited = BASE_SRC.replace(
        "store count: Cell[Int]",
        "store count: Cell[Int]\n  store total: Cell[Int]",
    );
    assert_ne!(
        edited, BASE_SRC,
        "the fixture edit must actually add a field"
    );

    let after = canonical_for(&edited);
    assert_ne!(
        before, after,
        "adding a store field is a genuine signature change and must move UnitSignature's canonical form"
    );
}
