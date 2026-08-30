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
//! [DECISION B]: three companion negative cases — a `store`-field addition,
//! a parameter retype, a handler method-name rename — none a body edit,
//! proving the comparison isn't vacuously satisfied. PR #1518's own bot
//! review: one negative case alone only pins the one section it touches; a
//! rendering/comparison bug confined to `fns:` or `agent_handlers:` could
//! still pass a fixture whose only negative case moves `store:`. The same
//! "prove the probe isn't vacuous" discipline P8.0's own
//! `query_types_found_does_not_count_checkers_pre_existing_type_of`
//! regression test already modeled for a different probe, now applied per
//! section rather than once for the whole form.
//!
//! [DECISION C] / PR #1518's own bot review (finding #1): the positive
//! fixture must reach every section `canon_unit_signature` renders (types,
//! fns, methods, service_handlers, agent_handlers, service_protocols, store,
//! capabilities) — not just the three a minimal function+agent fixture
//! happens to populate — or the R3.14 claim is proven for some sections and
//! vacuously true for the rest. `BASE_SRC` below carries one of each: a
//! cross-context type (`Widget`, via `uses demo.shared`), a local type
//! (`Local`, so `types:` also covers a `TypeDecl` whose own span shifts),
//! a free function, a method (`Local.triple` — one of the three gaps
//! PR #1517's own review found missing from `UnitSignature` entirely), a
//! capability with a provider (`ProviderOp.body` sits alongside, present but
//! never edited — correctly excluded, same as any other body), an agent
//! with a `store` field and a handler, and a service handler consuming the
//! cross-context type. Three declarations carry a body and are the ones
//! edited (`add`, `Local.triple`, `Counter.increment`); every other
//! declaration sits after at least one of them in the file and so still has
//! every one of its own spans shift, without being edited itself — exactly
//! the "stable even though its own spans moved" claim R3.14 makes about the
//! *whole* unit, not just the parts a test bothers to touch directly.

use std::collections::HashMap;
use std::path::PathBuf;

use bynk_check::symbols::{build_unit_table, combined_types_for};
use bynk_check::unit_signature::unit_signature_for;
use bynk_project::{ParsedFile, UnitKind};
use bynk_syntax::ast::SourceUnit;

fn parsed(source_path: &str, src: &str, kind: UnitKind) -> ParsedFile {
    let tokens = bynk_syntax::lexer::tokenize(src).expect("lex");
    let unit = bynk_syntax::parser::parse_unit(&tokens, src).expect("parse");
    match (&unit, kind) {
        (SourceUnit::Commons(_), UnitKind::Commons)
        | (SourceUnit::Context(_), UnitKind::Context) => {}
        _ => unreachable!("fixture kind must match the parsed source"),
    }
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

const SHARED_SRC: &str = r#"commons demo.shared

type Widget = {
  id: String,
}
"#;

/// The fixture unit ([DECISION C]): one declaration per category
/// `canon_unit_signature` renders, with the three that carry a `body`
/// (`add`, `Local.triple`, `Counter.increment`) placed first so every later
/// declaration's own spans shift when one of them is edited.
const BASE_SRC: &str = r#"context demo.stability

uses demo.shared

exports capability { Notifier }

fn add(a: Int, b: Int) -> Int {
  a + b
}

type Local = {
  n: Int,
}

fn Local.triple(self) -> Int {
  self.n * 3
}

capability Notifier {
  fn ping(seed: Int) -> Effect[Int]
}

provides Notifier = StubNotifier {
  fn ping(seed: Int) -> Effect[Int] {
    Effect.pure(seed + 1)
  }
}

agent Counter {
  key id: String

  store count: Cell[Int]

  on call increment() -> Effect[Int] {
    count := count + 1
    Effect.pure(count)
  }
}

service api from http {
  on GET("/widgets/:id") (id: String) -> Effect[HttpResult[Widget]] by Visitor given Notifier {
    Ok(Widget { id: id })
  }
}
"#;

/// Builds a canonical `UnitSignature` rendering for `demo.stability` parsed
/// from `context_src`, against `demo.shared`'s own combined types (the
/// cross-context-types category).
fn canonical_for(context_src: &str) -> String {
    let shared = parsed("demo/shared.bynk", SHARED_SRC, UnitKind::Commons);
    let context = parsed("demo/stability.bynk", context_src, UnitKind::Context);

    let mut errors = Vec::new();
    let shared_table = build_unit_table(
        "demo.shared",
        UnitKind::Commons,
        &[0],
        &[shared],
        &mut errors,
    );
    let context_table = build_unit_table(
        "demo.stability",
        UnitKind::Context,
        &[0],
        &[context],
        &mut errors,
    );
    assert!(errors.is_empty(), "fixture must parse cleanly: {errors:?}");

    let mut unit_tables = HashMap::new();
    unit_tables.insert("demo.shared".to_string(), shared_table);
    unit_tables.insert("demo.stability".to_string(), context_table.clone());
    let mut unit_uses = HashMap::new();
    unit_uses.insert(
        "demo.stability".to_string(),
        vec!["demo.shared".to_string()],
    );

    let combined = combined_types_for("demo.stability", &unit_tables, &unit_uses);
    unit_signature_for("demo.stability", &context_table, combined).canonical()
}

/// The R3.14 proof itself ([DECISION A]/[DECISION C]): editing only the
/// three bodies in the fixture — a free function, a method, an agent
/// handler — must leave `UnitSignature`'s canonical rendering byte-for-byte
/// unchanged, across every section it renders, not just the three the edits
/// directly touch.
#[test]
fn unit_signature_is_stable_under_a_body_only_edit() {
    let before = canonical_for(BASE_SRC);

    // PR #1518's own bot review (finding #2): guard each replacement
    // individually against an intermediate binding, not just the final
    // result against `BASE_SRC` — otherwise one `.replace()` silently
    // becoming a no-op (a reformatted fixture, a shifted grammar) would
    // degrade this into a one- or two-edit test while staying green.
    let after_fn = BASE_SRC.replace("a + b\n}", "let sum = a + b\n  sum\n}");
    assert_ne!(
        after_fn, BASE_SRC,
        "the `add` body edit must actually match"
    );

    let after_method = after_fn.replace("self.n * 3\n}", "let m = self.n * 3\n  m\n}");
    assert_ne!(
        after_method, after_fn,
        "the `Local.triple` body edit must actually match"
    );

    let after_handler = after_method.replace(
        "count := count + 1\n    Effect.pure(count)",
        "count := count + 1 + 0\n    Effect.pure(count)",
    );
    assert_ne!(
        after_handler, after_method,
        "the `Counter.increment` body edit must actually match"
    );

    let after = canonical_for(&after_handler);
    assert_eq!(
        before, after,
        "editing only the fn/method/handler bodies in the fixture must not move UnitSignature's canonical form"
    );
}

/// [DECISION B], negative case 1: adding a new `store` field is never a body
/// edit — `Counter` gains real, externally-relevant state — so it MUST
/// change the canonical form (the `store:` section specifically).
#[test]
fn unit_signature_is_not_stable_under_a_store_field_addition() {
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

/// [DECISION B], negative case 2 (PR #1518's own bot review, finding #3):
/// retyping a parameter changes `add`'s own entry in `fns:` without
/// touching any body — a rendering bug confined to `fns:` would still let
/// negative case 1 above pass, since that one only ever moves `store:`.
#[test]
fn unit_signature_is_not_stable_under_a_parameter_retype() {
    let before = canonical_for(BASE_SRC);

    let edited = BASE_SRC.replacen("a: Int, b: Int", "a: String, b: Int", 1);
    assert_ne!(
        edited, BASE_SRC,
        "the fixture edit must actually retype a param"
    );

    let after = canonical_for(&edited);
    assert_ne!(
        before, after,
        "retyping a parameter is a genuine signature change and must move UnitSignature's canonical form"
    );
}

/// [DECISION B], negative case 3 (PR #1518's own bot review, finding #3):
/// renaming a handler's own method name changes its entry in
/// `agent_handlers:` without touching any body — pinning the section
/// neither of the other two negative cases reaches.
#[test]
fn unit_signature_is_not_stable_under_a_handler_method_rename() {
    let before = canonical_for(BASE_SRC);

    let edited = BASE_SRC.replacen("on call increment()", "on call bump()", 1);
    assert_ne!(
        edited, BASE_SRC,
        "the fixture edit must actually rename the handler"
    );

    let after = canonical_for(&edited);
    assert_ne!(
        before, after,
        "renaming a handler's method name is a genuine signature change and must move UnitSignature's canonical form"
    );
}
