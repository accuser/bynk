//! Drift guard: every file under `design/pending/` is valid against the format.
//!
//! This runs under `cargo nextest run --workspace` (the main `test` job, and the
//! local full gate) and under `cargo test -p xtask` (the `drift` job, for a
//! `design/pending/`-only PR that the `rust`-filtered `test` job would skip). So
//! a malformed pending file cannot reach `main` by either path.

#[test]
fn design_pending_files_are_valid() {
    match xtask::check_all() {
        Ok(_) => {}
        Err(errors) => panic!(
            "design/pending contains invalid pending-increment file(s):\n  {}\n\n\
             See design/pending/README.md for the format.",
            errors.join("\n  ")
        ),
    }
}
