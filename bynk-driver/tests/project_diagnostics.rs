//! #696: a project-level validation diagnostic must render with ariadne source
//! context — the same rich, boxed report a single-file parse/resolve error gets
//! — not the plain `[category] message` fallback. The regression went unnoticed
//! because `expected_error.txt` fixtures assert category strings only (ADR 0198),
//! so the rendering path had no coverage. This drives the real project pipeline
//! and renders the way `bynk_driver::print_project_failure` does.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use bynk_emit::project;

/// A throwaway on-disk project, removed on drop (including on panic).
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn scratch_project(tag: &str, files: &[(&str, &str)]) -> Scratch {
    let dir = std::env::temp_dir().join(format!(
        "bynk_696_{tag}_{}_{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    for (rel, body) in files {
        let p = dir.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, body).unwrap();
    }
    Scratch(dir)
}

/// A `consumes` cycle — a project-level diagnostic built in `graph.rs` — must be
/// attributed to a file that is present in the failure's snapshots, carry a real
/// span (not the `0..0` fallback), and render with ariadne source context.
#[test]
fn a_consumes_cycle_renders_with_ariadne_source_context() {
    let root = scratch_project(
        "cycle",
        &[
            (
                "src/commerce/a.bynk",
                "context commerce.a {\n  consumes commerce.b\n}\n",
            ),
            (
                "src/commerce/b.bynk",
                "context commerce.b {\n  consumes commerce.a\n}\n",
            ),
        ],
    );

    // Root the project exactly as every `check`/`compile` command does.
    let opts = bynk_driver::project_options(&root.0);
    let failure = match project::compile_project(&opts) {
        Err(f) => f,
        Ok(_) => panic!("a `consumes` cycle must fail the build"),
    };

    let snapshots: HashMap<&Path, &str> = failure
        .snapshots
        .iter()
        .map(|(p, t)| (p.as_path(), t.as_str()))
        .collect();

    let cycle = failure
        .errors
        .iter()
        .find(|e| e.error.category == "bynk.context.consumes_cycle")
        .expect("the build must report a consumes cycle");

    // #696: the diagnostic is attributed to a real source file …
    let path = cycle
        .source_path
        .as_deref()
        .expect("the consumes cycle must be attributed to a file, not left unattributed");
    // … whose text is in the snapshot map (what makes the CLI take the rich branch) …
    let text = snapshots
        .get(path)
        .expect("the attributed file must have a snapshot to render against");
    // … and anchored on a real clause span, not the `0..0` project-level fallback.
    assert!(
        cycle.error.span.start > 0,
        "the cycle must be anchored on a real `consumes` clause span, got {:?}",
        cycle.error.span,
    );

    // Render exactly as `print_project_failure`'s rich branch does.
    let label = path.to_string_lossy().replace('\\', "/");
    let rendered =
        bynk_render::render_errors_plain(std::slice::from_ref(&cycle.error), text, &label);

    // The ariadne report frames the source excerpt with `╭─[<file>:<line>:<col>]`;
    // the plain fallback (`[category] message`) never does.
    assert!(
        rendered.contains("╭─["),
        "expected an ariadne source-context report, got:\n{rendered}",
    );
    assert!(
        rendered.contains(&format!("{label}:")),
        "the report must locate the diagnostic in its source file, got:\n{rendered}",
    );
}

/// The common cross-file-label case (#696): `kind_conflict` is *always* between
/// two files, so its "first declared here" provenance points into a different
/// file than the one the error is attributed to. It must render as a **note**
/// carrying the other file's path — never a secondary label that would underline
/// this file's own text (or panic on a mid-codepoint offset). The `consumes`
/// cycle above has no secondary label, so it cannot exercise this path.
#[test]
fn a_cross_file_kind_conflict_renders_the_other_file_as_a_note() {
    // Two files of the multi-file unit `shop.orders`, in the same directory,
    // declaring it as different kinds — a kind conflict with no path/name or
    // directory violation to muddy the diagnostics.
    let root = scratch_project(
        "kindconflict",
        &[
            (
                "src/shop/orders/a.bynk",
                "commons shop.orders {\n  type T = Int\n}\n",
            ),
            ("src/shop/orders/b.bynk", "context shop.orders {\n}\n"),
        ],
    );

    let opts = bynk_driver::project_options(&root.0);
    let failure = match project::compile_project(&opts) {
        Err(f) => f,
        Ok(_) => panic!("a kind conflict must fail the build"),
    };

    let snapshots: HashMap<&Path, &str> = failure
        .snapshots
        .iter()
        .map(|(p, t)| (p.as_path(), t.as_str()))
        .collect();

    let conflict = failure
        .errors
        .iter()
        .find(|e| e.error.category == "bynk.project.kind_conflict")
        .expect("the build must report a kind conflict");

    let path = conflict
        .source_path
        .as_deref()
        .expect("the kind conflict must be attributed to a file");
    let text = snapshots
        .get(path)
        .expect("the attributed file must have a snapshot");

    let label = path.to_string_lossy().replace('\\', "/");
    let rendered =
        bynk_render::render_errors_plain(std::slice::from_ref(&conflict.error), text, &label);

    // Rich source context for the primary span …
    assert!(
        rendered.contains("╭─["),
        "expected an ariadne source-context report, got:\n{rendered}",
    );
    // … and the other file's provenance survives as a note (not a label): the
    // note names the *other* file, which is never the one we rendered against.
    assert!(
        rendered.contains("first declared as a") && rendered.contains(".bynk`"),
        "the first-declaration site must render as a note naming the other file, got:\n{rendered}",
    );
    let other = if label.ends_with("a.bynk") {
        "b.bynk"
    } else {
        "a.bynk"
    };
    assert!(
        rendered.contains(other),
        "the note must point at the *other* file (`{other}`), got:\n{rendered}",
    );
}
