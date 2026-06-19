//! v0.24 (ADR 0052): project-wide diagnostics — non-bailing, overlay-aware,
//! file-attributed. The before/after this proves: the context-file error
//! below is handler-level (`bynk.given.unused_capability`), which the old
//! single-file `diagnose` cannot produce at all (it resolves/checks
//! `Commons` units only).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/diagnose/clean_and_broken/src")
}

#[test]
fn context_diagnostic_is_attributed_to_its_file() {
    let result = bynkc::diagnose_project(&fixture_root(), &HashMap::new());

    // Every discovered file appears, clean ones with an empty list.
    let by_path: HashMap<_, _> = result
        .files
        .iter()
        .map(|f| (f.source_path.to_string_lossy().replace('\\', "/"), f))
        .collect();
    let broken = by_path
        .get("billing/charge.bynk")
        .expect("context file analysed");
    let clean = by_path
        .get("shop/util.bynk")
        .expect("clean commons analysed");

    assert!(
        broken
            .diagnostics
            .iter()
            .any(|d| d.error.category == "bynk.given.unused_capability"),
        "the handler-level diagnostic is attributed to the context file; got: {:?}",
        broken
            .diagnostics
            .iter()
            .map(|d| d.error.category)
            .collect::<Vec<_>>()
    );
    assert!(
        clean.diagnostics.is_empty(),
        "the clean file carries no diagnostics"
    );
    // The snapshot is the analysed text.
    assert!(broken.text.contains("given Clock"));
}

#[test]
fn overlay_changes_diagnostics_without_touching_disk() {
    // Fix the unused capability in the overlay only: the buffer USES Clock.
    let fixed = "\
context billing.charge

capability Clock {
  fn now() -> Effect[Int]
}

provides Clock = FixedClock {
  fn now() -> Effect[Int] {
    42
  }
}

service charge {
  on call(cents: Int) -> Effect[Result[Int, ()]] given Clock {
    let t <- Clock.now()
    Ok(cents + t)
  }
}
";
    let abs = fixture_root().join("billing/charge.bynk");
    let canonical = abs.canonicalize().unwrap_or(abs);
    let mut overlay = HashMap::new();
    overlay.insert(canonical, fixed.to_string());

    let result = bynkc::diagnose_project(&fixture_root(), &overlay);
    let broken = result
        .files
        .iter()
        .find(|f| f.source_path.to_string_lossy().replace('\\', "/") == "billing/charge.bynk")
        .expect("context file analysed");
    assert!(
        broken.diagnostics.is_empty(),
        "overlay fix clears the diagnostic; got: {:?}",
        broken
            .diagnostics
            .iter()
            .map(|d| d.error.category)
            .collect::<Vec<_>>()
    );
    // The snapshot reflects the overlay, not the disk.
    assert!(broken.text.contains("Clock.now()"));
}

#[test]
fn analysis_does_not_bail_at_the_first_failure() {
    // Break BOTH files via overlays: a parse error in one, a semantic error
    // in the other. The old compile_project contract reports only the first
    // phase's failure; diagnose_project must report both, each attributed.
    let root = fixture_root();
    let mut overlay = HashMap::new();
    let util = root.join("shop/util.bynk");
    overlay.insert(
        util.canonicalize().unwrap_or(util),
        "commons shop.util\n\nfn broken( -> Int {\n  1\n}\n".to_string(),
    );

    let result = bynkc::diagnose_project(&root, &overlay);
    let cats: Vec<(String, &'static str)> = result
        .files
        .iter()
        .flat_map(|f| {
            let p = f.source_path.to_string_lossy().replace('\\', "/");
            f.diagnostics
                .iter()
                .map(move |d| (p.clone(), d.error.category))
        })
        .collect();
    assert!(
        cats.iter()
            .any(|(p, c)| p == "shop/util.bynk" && c.starts_with("bynk.parse")),
        "parse error attributed to the broken commons; got {cats:?}"
    );
    assert!(
        cats.iter()
            .any(|(p, c)| p == "billing/charge.bynk" && *c == "bynk.given.unused_capability"),
        "semantic error still produced for the other unit; got {cats:?}"
    );
}
