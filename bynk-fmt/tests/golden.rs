//! Golden-output tests for the Bynk formatter.
//!
//! Each fixture under `tests/fixtures/<case>/` pairs a deliberately
//! mis-formatted `input.bynk` with the canonical `expected.bynk` the formatter
//! must produce. Unlike the round-trip suite (which only checks idempotency and
//! re-parsing — and so is blind to formatting that is *wrong but stable*), this
//! suite pins the exact canonical rendering of every construct. A formatting
//! change shows up as a reviewable diff in `expected.bynk` rather than silently
//! passing.
//!
//! For each fixture we assert:
//!  1. `format(input)  == expected`   — the canonical transformation.
//!  2. `format(expected) == expected` — `expected` is itself canonical (idempotent).
//!  3. `expected` tokenises and parses — formatting introduces no syntax errors.
//!
//! Regenerate the `expected.bynk` files after an intentional formatter change:
//!
//!     BYNK_BLESS=1 cargo test -p bynk-fmt --test golden
//!
//! Blessed output is reviewed by hand in the diff — a wrong bless is caught in
//! review, not ratified silently (the failure mode of an equality-only guard).

use std::fs;
use std::path::{Path, PathBuf};

use bynk_fmt::{FormatOptions, format_source};
use bynkc::lexer::tokenize;
use bynkc::parser::parse_unit;

/// All fixture directories under `tests/fixtures/`, sorted by name.
fn fixture_dirs() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures");
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(&root) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() && p.join("input.bynk").is_file() {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

fn case_name(dir: &Path) -> String {
    dir.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("?")
        .to_string()
}

/// A minimal unified-ish diff: the first differing line plus a little context,
/// enough to locate a formatting regression without pulling in a diff crate.
fn first_difference(expected: &str, got: &str) -> String {
    let exp: Vec<&str> = expected.lines().collect();
    let gat: Vec<&str> = got.lines().collect();
    for (i, (e, g)) in exp.iter().zip(gat.iter()).enumerate() {
        if e != g {
            return format!(
                "first difference at line {}:\n  expected: {:?}\n  got:      {:?}",
                i + 1,
                e,
                g
            );
        }
    }
    if exp.len() != gat.len() {
        return format!(
            "line count differs: expected {} lines, got {} lines",
            exp.len(),
            gat.len()
        );
    }
    "outputs differ only in trailing whitespace or newline".to_string()
}

#[test]
fn golden_fixtures_format_canonically() {
    let opts = FormatOptions::default();
    let bless = std::env::var_os("BYNK_BLESS").is_some();
    let dirs = fixture_dirs();
    assert!(
        !dirs.is_empty(),
        "no golden fixtures found under bynk-fmt/tests/fixtures/"
    );

    let mut failures = Vec::new();
    let mut blessed = 0usize;

    for dir in &dirs {
        let name = case_name(dir);
        let input_path = dir.join("input.bynk");
        let expected_path = dir.join("expected.bynk");

        let input = match fs::read_to_string(&input_path) {
            Ok(s) => s,
            Err(e) => {
                failures.push(format!("{name}: cannot read input.bynk: {e}"));
                continue;
            }
        };

        let got = match format_source(&input, &opts) {
            Ok(s) => s,
            Err(e) => {
                failures.push(format!(
                    "{name}: formatting input.bynk failed with {} error(s)",
                    e.errors.len()
                ));
                continue;
            }
        };

        if bless {
            if let Err(e) = fs::write(&expected_path, &got) {
                failures.push(format!("{name}: cannot write expected.bynk: {e}"));
            } else {
                blessed += 1;
            }
            continue;
        }

        let expected = match fs::read_to_string(&expected_path) {
            Ok(s) => s,
            Err(e) => {
                failures.push(format!(
                    "{name}: cannot read expected.bynk ({e}) — \
                     regenerate with BYNK_BLESS=1 cargo test -p bynk-fmt --test golden"
                ));
                continue;
            }
        };

        // 1. Canonical transformation.
        if got != expected {
            failures.push(format!(
                "{name}: format(input) != expected\n{}\n  \
                 (regenerate with BYNK_BLESS=1 if this change is intended)",
                first_difference(&expected, &got)
            ));
        }

        // 2. `expected` is itself canonical (idempotent).
        match format_source(&expected, &opts) {
            Ok(reformatted) if reformatted == expected => {}
            Ok(reformatted) => failures.push(format!(
                "{name}: expected.bynk is not canonical (format(expected) != expected)\n{}",
                first_difference(&expected, &reformatted)
            )),
            Err(e) => failures.push(format!(
                "{name}: formatting expected.bynk failed with {} error(s)",
                e.errors.len()
            )),
        }

        // 3. `expected` parses.
        match tokenize(&expected) {
            Ok(tokens) => {
                if let Err(errs) = parse_unit(&tokens, &expected) {
                    failures.push(format!(
                        "{name}: expected.bynk does not parse ({} error(s); first: {})",
                        errs.len(),
                        errs.first().map(|e| e.message.as_str()).unwrap_or("?")
                    ));
                }
            }
            Err(e) => failures.push(format!(
                "{name}: expected.bynk does not tokenise: {}",
                e.message
            )),
        }
    }

    if bless {
        eprintln!("blessed {blessed} golden fixture(s)");
    }
    assert!(
        failures.is_empty(),
        "golden formatter fixtures failed ({}):\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}
