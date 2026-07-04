//! Rendering for in-process compiles — the driver's flatten-then-delegate layer.
//!
//! `bynk dev`, `bynk check`, and any other command that links the pipeline share
//! one copy of the ProjectFailure → CompileError flattening (ADR 0100): attribute
//! each error to its file snapshot **here, in the front-end**, and delegate the
//! per-error rendering to `bynk-render`. The flattening stays above `bynk-render`
//! so there is no `render → emit` edge. Mirrors `bynkc`'s `print_project_failure`
//! / `print_project_failure_short` / `print_project_warnings` so an in-process
//! `bynk check` renders identically to `bynkc check`.

use std::collections::HashMap;
use std::path::Path;

use bynk_emit::project::{AttributedError, ProjectFailure};

/// Render a project compile failure with full ariadne source context. An
/// unattributed (project-level) error keeps the plain `[category] message` form.
pub fn render_project_failure(failure: &ProjectFailure) {
    let texts = snapshot_map(failure);
    for ae in &failure.errors {
        match attributed_text(ae, &texts) {
            Some((path, text)) => {
                let label = path.to_string_lossy().replace('\\', "/");
                bynk_render::print_errors(std::slice::from_ref(&ae.error), text, &label);
            }
            None => {
                eprintln!("[{}] {}", ae.error.category, ae.error.message);
                for note in &ae.error.notes {
                    eprintln!("  note: {note}");
                }
            }
        }
    }
}

/// The `--format short` analogue: one `path:line:col: severity[category]:
/// message` line per attributed error; an unattributed project-level error
/// falls back to `severity[category]: message`. Matches `bynkc check --format
/// short` (whose output the VS Code `bynkc` problem-matcher re-parses).
pub fn render_project_failure_short(failure: &ProjectFailure) {
    let texts = snapshot_map(failure);
    for ae in &failure.errors {
        let line = match attributed_text(ae, &texts) {
            Some((path, text)) => {
                let label = path.to_string_lossy().replace('\\', "/");
                bynk_render::short_line(&label, text, &ae.error)
            }
            None => format!(
                "{}[{}]: {}",
                bynk_render::severity_word(&ae.error),
                ae.error.category,
                ae.error.message
            ),
        };
        eprintln!("{line}");
    }
}

/// Print a successful build's non-failing warnings (ADR 0117). A successful
/// build keeps no per-file snapshots, so warnings render in the plain
/// `warning[<category>]: <message>` form (with the owning file, when known).
pub fn print_project_warnings(warnings: &[AttributedError]) {
    for w in warnings {
        let where_ = w
            .source_path
            .as_deref()
            .map(|p| format!("{}: ", p.to_string_lossy().replace('\\', "/")))
            .unwrap_or_default();
        eprintln!("{where_}warning[{}]: {}", w.error.category, w.error.message);
        for note in &w.error.notes {
            eprintln!("  note: {note}");
        }
    }
}

/// The file snapshots a failure carries, keyed by path for attribution lookups.
fn snapshot_map(failure: &ProjectFailure) -> HashMap<&Path, &str> {
    failure
        .snapshots
        .iter()
        .map(|(p, t)| (p.as_path(), t.as_str()))
        .collect()
}

/// The `(path, source)` an attributed error renders against, when its owning
/// file's snapshot is present; `None` for a project-level error.
fn attributed_text<'a>(
    ae: &'a AttributedError,
    texts: &HashMap<&'a Path, &'a str>,
) -> Option<(&'a Path, &'a str)> {
    ae.source_path
        .as_deref()
        .and_then(|p| texts.get(p).map(|t| (p, *t)))
}
