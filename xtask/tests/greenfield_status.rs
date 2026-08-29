//! Drift guard: `design/greenfield-status.md` matches a fresh probe run.
//!
//! Only the fourteen gated (zero/closure/existence) probes are diffed — `wildcard_arms`,
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
    let disagreements = xtask::greenfield_status::gated_disagreements(&root);
    assert!(
        disagreements.is_empty(),
        "design/greenfield-status.md is stale — run `cargo xtask greenfield-status --apply`:\n{}",
        disagreements
            .iter()
            .map(|(name, committed, live)| format!(
                "  {name}: committed {committed:?}, live {live:?}"
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
