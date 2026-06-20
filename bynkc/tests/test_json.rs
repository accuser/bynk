//! v0.59: the `bynkc test --format json` surface.
//!
//! Two toolchain-free layers (proposal v0.59 "Goldening strategy"):
//!  1. **The pinned document** — built from a synthetic [`TestRun`] model and
//!     compared against a committed golden (the `bynk doctor` precedent), for
//!     the normal / compile / runtime shapes.
//!  2. **The NDJSON → document parser** — fed fixture event streams (including a
//!     truncated/crashed one), asserting the folded result.
//!
//! The true end-to-end (node actually emitting NDJSON) is exercised by the
//! toolchain-gated suites, not here, so this stays deterministic on CI.
//!
//! Document goldens are blessed with `BYNK_BLESS=1 cargo test -p bynkc --test test_json`.

use std::path::Path;

use bynkc::test_json::{Case, Location, Suite, TestRun, parse_ndjson};

fn bless_or_assert(name: &str, actual: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    if std::env::var_os("BYNK_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "missing golden {}; regenerate with BYNK_BLESS=1 cargo test -p bynkc --test test_json",
            path.display()
        )
    });
    assert_eq!(
        actual, expected,
        "golden {name} drifted; re-bless with BYNK_BLESS=1 cargo test -p bynkc --test test_json"
    );
}

// Fixture event streams (the runner's internal NDJSON protocol).
const NDJSON_COMPLETE: &str = r#"{"type":"run-begin","suites":1}
{"type":"suite-begin","name":"commerce.payment","kind":"unit","tests":2}
{"type":"case","suite":"commerce.payment","name":"charges the card","outcome":"pass"}
{"type":"case","suite":"commerce.payment","name":"rejects expired card","outcome":"fail","message":"assertion failed at tests/commerce/payment.test.bynk:42:5","location":"tests/commerce/payment.test.bynk:42:5"}
{"type":"suite-end","name":"commerce.payment"}
{"type":"run-end","passed":1,"failed":1}
"#;

// A runner that died mid-suite: a complete prefix, but no `run-end`.
const NDJSON_TRUNCATED: &str = r#"{"type":"run-begin","suites":2}
{"type":"suite-begin","name":"orders","kind":"integration","tests":3}
{"type":"case","suite":"orders","name":"places an order","outcome":"pass"}
"#;

// ---------------------------------------------------------------------------
// Document goldens (synthetic model → JSON)
// ---------------------------------------------------------------------------

#[test]
fn golden_document_normal() {
    let run = TestRun {
        passed: 1,
        failed: 1,
        suites: Some(vec![Suite {
            name: "commerce.payment".to_string(),
            kind: "unit".to_string(),
            cases: vec![
                Case {
                    name: "charges the card".to_string(),
                    outcome: "pass".to_string(),
                    message: None,
                    location: None,
                },
                Case {
                    name: "rejects expired card".to_string(),
                    outcome: "fail".to_string(),
                    message: Some(
                        "assertion failed at tests/commerce/payment.test.bynk:42:5".to_string(),
                    ),
                    location: Some(Location {
                        path: "tests/commerce/payment.test.bynk".to_string(),
                        line: 42,
                        col: 5,
                    }),
                },
            ],
        }]),
        error: None,
    };
    bless_or_assert("test-json-normal.json", &run.render());
}

#[test]
fn golden_document_compile() {
    let run = TestRun::compile_error(vec![
        "src/commerce/payment.bynk:3:5: error[bynk.types.mismatch]: expected `Money`, found `Int`"
            .to_string(),
    ]);
    bless_or_assert("test-json-compile.json", &run.render());
}

#[test]
fn golden_document_runtime() {
    // A crashed run: the observed prefix is kept, plus a `runtime` error.
    let doc = parse_ndjson(NDJSON_TRUNCATED).into_document("RangeError: out of memory\n");
    bless_or_assert("test-json-runtime.json", &doc.render());
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

#[test]
fn parser_folds_a_complete_run() {
    let run = parse_ndjson(NDJSON_COMPLETE);
    assert!(run.complete, "a stream with run-end is complete");
    assert_eq!(run.passed, 1);
    assert_eq!(run.failed, 1);
    assert_eq!(run.suites.len(), 1);
    let suite = &run.suites[0];
    assert_eq!(suite.name, "commerce.payment");
    assert_eq!(suite.kind, "unit");
    assert_eq!(suite.cases.len(), 2);
    let fail = &suite.cases[1];
    assert_eq!(fail.outcome, "fail");
    assert_eq!(
        fail.location,
        Some(Location {
            path: "tests/commerce/payment.test.bynk".to_string(),
            line: 42,
            col: 5,
        })
    );
}

#[test]
fn parser_truncated_stream_is_incomplete_and_becomes_runtime_error() {
    let run = parse_ndjson(NDJSON_TRUNCATED);
    assert!(!run.complete, "no run-end ⇒ incomplete");
    assert_eq!(run.passed, 1);
    assert_eq!(run.suites.len(), 1, "the prefix suite is kept");

    let doc = run.into_document("boom\n");
    let err = doc.error.expect("a crashed run carries a runtime error");
    assert_eq!(err.kind, "runtime");
    assert_eq!(err.stderr.as_deref(), Some("boom"));
    assert!(doc.suites.is_some(), "the observed prefix is preserved");
}

#[test]
fn parser_complete_run_has_no_error() {
    let doc = parse_ndjson(NDJSON_COMPLETE).into_document("");
    assert!(doc.error.is_none(), "a complete run is not an error");
    assert_eq!(doc.failed, 1, "failing assertions are not a runtime error");
}

#[test]
fn parser_skips_unparseable_and_unknown_lines() {
    let stream = "not json at all\n{\"type\":\"run-begin\",\"suites\":1}\n{\"type\":\"mystery\"}\n{\"type\":\"run-end\",\"passed\":0,\"failed\":0}\n";
    let run = parse_ndjson(stream);
    assert!(run.complete);
    assert_eq!(run.suites.len(), 0);
}

#[test]
fn parser_unknown_location_yields_no_structured_location() {
    // A non-assertion throw carries `location: "unknown"`, which is not a
    // path:line:col — the case keeps its message but offers no click-through.
    let stream = "{\"type\":\"suite-begin\",\"name\":\"s\",\"kind\":\"unit\",\"tests\":1}\n{\"type\":\"case\",\"suite\":\"s\",\"name\":\"boom\",\"outcome\":\"fail\",\"message\":\"TypeError: x\",\"location\":\"unknown\"}\n{\"type\":\"run-end\",\"passed\":0,\"failed\":1}\n";
    let run = parse_ndjson(stream);
    let case = &run.suites[0].cases[0];
    assert_eq!(case.message.as_deref(), Some("TypeError: x"));
    assert_eq!(case.location, None);
}
