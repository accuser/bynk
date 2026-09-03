//! Drift guard: #1560 — `bynk-testkit` is `publish = false` and only ever a
//! dev-dependency, so `cargo publish` for its three consumers depends on two
//! invariants nothing else enforces:
//!
//! 1. its `[workspace.dependencies]` entry carries no `version` (a versioned
//!    dependency must resolve against crates.io even with `--no-verify`, and
//!    `bynk-testkit` can never be there — see the entry's own comment in the
//!    root `Cargo.toml`);
//! 2. no crate lists it under `[dependencies]` (a path-only *normal*
//!    dependency, unlike a dev one, is a hard `cargo publish` error: "all
//!    dependencies must have a version").
//!
//! Before this guard, only a comment stood between either invariant and a
//! `cargo publish` failure discovered mid-release — the single most expensive
//! place to find it. Review of #1593.

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// The `bynk-testkit = { ... }` line from the root `Cargo.toml`'s
/// `[workspace.dependencies]` table.
fn testkit_workspace_dependency_line(root: &std::path::Path) -> String {
    let cargo = std::fs::read_to_string(root.join("Cargo.toml")).expect("read root Cargo.toml");
    cargo
        .lines()
        .find(|line| line.trim_start().starts_with("bynk-testkit = {"))
        .expect("root Cargo.toml's [workspace.dependencies] has no bynk-testkit entry")
        .to_string()
}

#[test]
fn testkit_workspace_dependency_has_no_version() {
    let line = testkit_workspace_dependency_line(&repo_root());
    assert!(
        !line.contains("version"),
        "bynk-testkit's [workspace.dependencies] entry carries a `version` again: \
         {line:?} — this makes `cargo publish` fail unconditionally (regardless of \
         --no-verify) for every crate that depends on it, since bynk-testkit is \
         `publish = false` and can never resolve against crates.io (#1560)"
    );
}

/// Every workspace member whose `Cargo.toml` names `bynk-testkit.workspace = true`,
/// and the `[section]` header it appears under.
fn testkit_dependents(root: &std::path::Path) -> Vec<(String, String)> {
    let cargo = std::fs::read_to_string(root.join("Cargo.toml")).expect("read root Cargo.toml");
    let start = cargo
        .find("members = [")
        .expect("no [workspace] members array")
        + "members = [".len();
    let end = start
        + cargo[start..]
            .find(']')
            .expect("unterminated members array");
    let members: Vec<String> = cargo[start..end]
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.trim_end_matches(',').trim_matches('"').to_string())
        .collect();

    let mut found = Vec::new();
    for member in members {
        let manifest_path = root.join(&member).join("Cargo.toml");
        let Ok(manifest) = std::fs::read_to_string(&manifest_path) else {
            continue;
        };
        let mut section = String::new();
        for line in manifest.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                section = trimmed.to_string();
                continue;
            }
            if trimmed == "bynk-testkit.workspace = true" {
                found.push((member.clone(), section.clone()));
            }
        }
    }
    found
}

#[test]
fn testkit_is_a_dev_dependency_everywhere_it_appears() {
    let dependents = testkit_dependents(&repo_root());
    assert!(
        !dependents.is_empty(),
        "no workspace member references bynk-testkit.workspace = true any more — if \
         it's genuinely unused, drop it from [workspace.dependencies] and this test; \
         if this is a parsing bug, fix the scan instead of deleting the assertion"
    );
    for (member, section) in &dependents {
        assert_eq!(
            section, "[dev-dependencies]",
            "{member}/Cargo.toml lists bynk-testkit.workspace = true under {section}, \
             not [dev-dependencies] — a path-only *normal* dependency (which is what \
             bynk-testkit is, per #1560) is a hard `cargo publish` error: \"all \
             dependencies must have a version\""
        );
    }
}
