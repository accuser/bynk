//! v0.177 (#643): standing guards on the cross-context contract hash.
//!
//! The hash's danger is not that it fails to catch skew — it is that it fires
//! when there *is* no skew. A spurious 409 breaks a working deployment and
//! destroys trust in the mechanism, which is worse than having no check at all.
//! Every plausible cause of one is a canonicalisation bug, so these guards test
//! that side hardest.

use std::collections::HashMap;
use std::path::Path;

/// Every caller's stamped hash equals the constant its callee compares against.
///
/// This is the no-false-positive property, checked over the whole blessed
/// corpus rather than argued: caller and callee canonicalise the callee's
/// contract from tables built by the same `combined_types_for`, so on a single
/// build they must agree for *every* service in *every* fixture. A single
/// mismatch here means the emitter ships a boundary that 409s on first contact.
#[test]
fn every_stamped_hash_matches_its_callees_constant() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/positive");
    let stamp =
        regex::Regex::new(r#"callService\([^)]*?"([A-Za-z_][A-Za-z0-9_]*)",.*?"([0-9a-f]{16})"\)"#)
            .unwrap();
    let expect = regex::Regex::new(
        r#"kind: "ContractMismatch", service: "([^"]+)", expected: "([0-9a-f]{16})""#,
    )
    .unwrap();

    let mut expected: HashMap<(String, String), String> = HashMap::new();
    let mut stamped: Vec<(String, String, String)> = Vec::new();

    let Ok(entries) = std::fs::read_dir(&root) else {
        panic!("fixture root not readable: {}", root.display());
    };
    for e in entries.flatten() {
        let fx = e.file_name().to_string_lossy().to_string();
        let workers = e.path().join("expected/workers");
        let Ok(dirs) = std::fs::read_dir(&workers) else {
            continue;
        };
        for d in dirs.flatten() {
            if let Ok(s) = std::fs::read_to_string(d.path().join("handlers.ts")) {
                for c in stamp.captures_iter(&s) {
                    stamped.push((fx.clone(), c[1].to_string(), c[2].to_string()));
                }
            }
            if let Ok(s) = std::fs::read_to_string(d.path().join("index.ts")) {
                for c in expect.captures_iter(&s) {
                    expected.insert((fx.clone(), c[1].to_string()), c[2].to_string());
                }
            }
        }
    }

    assert!(
        !stamped.is_empty(),
        "no cross-context call sites found — the guard would pass vacuously"
    );

    let mut problems: Vec<String> = Vec::new();
    for (fx, svc, h) in &stamped {
        match expected.get(&(fx.clone(), svc.clone())) {
            None => problems.push(format!(
                "{fx}/{svc}: caller stamps {h} but the callee emits no contract check"
            )),
            Some(e) if e != h => problems.push(format!(
                "{fx}/{svc}: caller stamps {h} but the callee expects {e} — \
                 a spurious 409 on every call"
            )),
            Some(_) => {}
        }
    }
    assert!(
        problems.is_empty(),
        "contract hashes disagree within a single build:\n{}",
        problems.join("\n")
    );
}

/// Compile a two-context workers project and return the hash `a` stamps for
/// `b.probe`, read out of the emitted caller.
fn stamped_hash(b_body: &str, tag: &str) -> String {
    use bynkc::{BuildTarget, CompileOptions};
    let tmp = std::env::temp_dir().join(format!("bynk-contract-nf-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    // Flat layout (no `src/`), matching `cross_context_caller.rs`: the project
    // root *is* the include root, so `app/b.bynk` declares `app.b`.
    let proj = tmp.join("proj/app");
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(proj.join("b.bynk"), b_body).unwrap();
    std::fs::write(
        proj.join("a.bynk"),
        "context app.a\n\nconsumes app.b as B\n\nservice ask {\n  on call(x: String) -> Effect[Result[String, String]] {\n    let r <- B.probe(x)\n    r\n  }\n}\n",
    )
    .unwrap();
    let out = bynkc::compile_project(
        &CompileOptions::single(tmp.join("proj")).target(BuildTarget::Workers),
    )
    .unwrap_or_else(|f| {
        panic!(
            "compile {tag}:\n{}",
            bynkc::render_project_errors(&f.flatten())
        )
    });
    let caller = out
        .files
        .iter()
        .find(|f| f.output_path.ends_with("workers/app-a/handlers.ts"))
        .expect("caller emitted");
    let h = regex::Regex::new(r#"callService\([^)]*?"([0-9a-f]{16})"\)"#)
        .unwrap()
        .captures(&caller.typescript)
        .map(|c| c[1].to_string())
        .unwrap_or_else(|| panic!("no stamped hash in {tag}:\n{}", caller.typescript));
    let _ = std::fs::remove_dir_all(&tmp);
    h
}

/// A **parameter rename** is a wire change, not a refactor: the multi-argument
/// wire is an object keyed by parameter name.
///
/// End-to-end rather than over the normal form directly, because it is one of
/// the few contract changes a *co-compiled* pair does not already reject: the
/// caller passes positionally, so only the callee's spelling moves. Most other
/// changes (a renamed record field, a retyped parameter) are caught by the
/// checker's structural rule long before a hash could speak — which is the point
/// of the hash living where it does. It exists for the pair that was *never*
/// co-compiled: A built at rev1, B redeployed at rev2. Those cases are covered
/// over the normal form itself, in `bynk-check`'s `contract` unit tests, since no
/// single build can express them.
#[test]
fn a_parameter_rename_changes_the_hash() {
    let a = stamped_hash(
        "context app.b\n\nservice probe {\n  on call(x: String) -> Effect[Result[String, String]] {\n    Ok(x)\n  }\n}\n",
        "param-x",
    );
    // The caller calls `B.probe(x)` positionally, so only the callee's parameter
    // name differs — which is exactly the case a name-blind form would miss.
    let b = stamped_hash(
        "context app.b\n\nservice probe {\n  on call(renamed: String) -> Effect[Result[String, String]] {\n    Ok(renamed)\n  }\n}\n",
        "param-renamed",
    );
    assert_ne!(a, b, "a parameter rename must change the hash");
}
