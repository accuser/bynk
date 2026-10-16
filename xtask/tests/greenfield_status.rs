//! Drift guard: `design/greenfield-status.md` matches a fresh probe run.
//!
//! Only the sixteen gated (zero/closure/existence) probes are diffed — `wildcard_arms`,
//! `keep_in_sync`, `test_density`, `fixture_kinds`, `keystroke_latency` are trend
//! probes, recomputed and printed by `cargo xtask greenfield-status` but never gated
//! here (#999 Decision D): they move on nearly any ordinary Rust PR (or, for
//! `keystroke_latency`, are settled to not move at all this phase — see its own doc
//! comment), and hard-gating them would make this file churn, and conflict, on
//! routine work.
//!
//! This runs under `cargo nextest run --workspace` (the main `test` job, for any
//! Rust-touching PR — fifteen of the nineteen probes read Rust source) and under
//! `cargo test -p xtask` (the `drift` job, for the pending/decisions-only PRs the
//! `rust`-filtered `test` job skips). A `drift`-job *step* alone would have missed the
//! former; a `#[test]` here rides both paths with no new CI wiring.

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

#[test]
fn greenfield_status_table_is_current() {
    let root = repo_root();
    let probes = xtask::greenfield_status::run_gated(&root);

    // A pending file's `probes` key is validated against GATED_PROBES; pin that
    // list to the probes that actually run so the two can never drift.
    let names: Vec<&str> = probes.iter().map(|p| p.name).collect();
    assert_eq!(
        names,
        xtask::greenfield_status::GATED_PROBES,
        "GATED_PROBES is out of step with run_gated()"
    );

    let disagreements = xtask::greenfield_status::gated_disagreements_in(&probes, &root);
    let acknowledged = xtask::acknowledged_probes(&root);
    for (name, committed, live) in &disagreements {
        if acknowledged.contains(name) {
            eprintln!(
                "greenfield-status: {name} moves {committed:?} -> {live:?} \
                 (acknowledged by a pending file; the stamp rewrites the row on merge)"
            );
        }
    }

    let unacknowledged = xtask::greenfield_status::unacknowledged(disagreements, &acknowledged);
    assert!(
        unacknowledged.is_empty(),
        "gated greenfield-status probe(s) moved without acknowledgement. Either name them in \
         your increment's `design/pending/<slug>.md` (`probes: a, b` — preferred: the stamp \
         rewrites the table on merge, so parallel PRs don't conflict on it), or, for a change \
         with no increment, run `cargo xtask greenfield-status --apply`:\n{}",
        unacknowledged
            .iter()
            .map(|(name, committed, live)| format!(
                "  {name}: committed {committed:?}, live {live:?}"
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
