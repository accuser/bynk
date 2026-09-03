//! Drift guard: #1559 — `release.yml` and `release-bootstrap.yml` each hardcode
//! their own `for crate in ...; do` publish list rather than deriving it from the
//! workspace, so a new publishable crate (or one that stops being publishable)
//! can silently drift out of sync with both. `bynk-project` was exactly that: a
//! real, publishable dependency of `bynk-check`/`bynk-emit`/`bynk-ide`/
//! `bynk-lower`, missing from both lists since its 30 July 2026 extraction,
//! unnoticed because nothing exercised a real release in the meantime. This
//! guard makes crate #17 (or #16, if one is ever retired) fail loud here instead
//! of at the next `cargo publish`, the same "fail loud, not quiet" discipline
//! `xtask::greenfield_status`'s own exception-list guards use (#1587).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// The workspace members declared in the root `Cargo.toml`'s `[workspace]
/// members` array, in declaration order — a straight-line parse of the quoted
/// strings between `members = [` and the matching `]`. No `toml` dependency
/// needed for a format this regular; mirrors `greenfield_status.rs`'s own
/// text-scan treatment of `Cargo.toml` elsewhere in this crate.
fn workspace_members(root: &Path) -> Vec<String> {
    let cargo = std::fs::read_to_string(root.join("Cargo.toml")).expect("read root Cargo.toml");
    let start = cargo
        .find("members = [")
        .expect("root Cargo.toml has no [workspace] members array")
        + "members = [".len();
    let end = start
        + cargo[start..]
            .find(']')
            .expect("root Cargo.toml's members array is unterminated");
    cargo[start..end]
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.trim_end_matches(',').trim_matches('"').to_string())
        .collect()
}

/// Does `member`'s own `Cargo.toml` declare `publish = false`?
fn is_unpublishable(root: &Path, member: &str) -> bool {
    let manifest = std::fs::read_to_string(root.join(member).join("Cargo.toml"))
        .unwrap_or_else(|e| panic!("read {member}/Cargo.toml: {e}"));
    manifest
        .lines()
        .any(|line| line.trim() == "publish = false")
}

/// The `for crate in ...; do` list a release workflow hardcodes — the exact
/// loop both `release.yml:334` and `release-bootstrap.yml:121` named at #1559.
fn workflow_publish_list(root: &Path, workflow: &str) -> Vec<String> {
    let path = root.join(".github/workflows").join(workflow);
    let yaml = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    let line = yaml
        .lines()
        .find(|line| line.trim_start().starts_with("for crate in "))
        .unwrap_or_else(|| panic!("{workflow} has no `for crate in ...` publish loop"));
    line.trim_start()
        .trim_start_matches("for crate in ")
        .split("; do")
        .next()
        .expect("`for crate in ...` line has no `; do`")
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

const RELEASE_WORKFLOWS: [&str; 2] = ["release.yml", "release-bootstrap.yml"];

/// Set equality both ways: a workspace member missing from a workflow's list
/// (#1559's own failure) fails, and so does a workflow list padded with a
/// crate no longer in the workspace (or one that gained `publish = false`).
#[test]
fn release_workflows_publish_exactly_the_publishable_workspace_members() {
    let root = repo_root();
    let publishable: BTreeSet<String> = workspace_members(&root)
        .into_iter()
        .filter(|member| !is_unpublishable(&root, member))
        .collect();
    for workflow in RELEASE_WORKFLOWS {
        let listed: BTreeSet<String> = workflow_publish_list(&root, workflow).into_iter().collect();
        assert_eq!(
            listed,
            publishable,
            "{workflow}'s hardcoded publish list has drifted from the workspace's \
             publishable members — missing: {:?}, stale: {:?}",
            publishable.difference(&listed).collect::<Vec<_>>(),
            listed.difference(&publishable).collect::<Vec<_>>()
        );
    }
}

/// `cargo publish -p X` fails if a workspace dependency of `X` isn't already on
/// crates.io, so each list must publish every crate after every in-workspace
/// dependency it names via `<dep>.workspace = true` — the ordering half of the
/// same #1559 shape (`bynk-project` itself was inserted right after
/// `bynk-syntax`, its only workspace dependency, and ahead of its four
/// dependents; this pins that relationship so it can't quietly invert).
#[test]
fn release_workflow_publish_lists_are_topologically_sound() {
    let root = repo_root();
    for workflow in RELEASE_WORKFLOWS {
        let list = workflow_publish_list(&root, workflow);
        for (i, krate) in list.iter().enumerate() {
            let manifest = std::fs::read_to_string(root.join(krate).join("Cargo.toml"))
                .unwrap_or_else(|e| panic!("read {krate}/Cargo.toml: {e}"));
            for (j, dep) in list.iter().enumerate() {
                if dep == krate {
                    continue;
                }
                let needle = format!("{dep}.workspace = true");
                assert!(
                    !manifest.contains(&needle) || j < i,
                    "{workflow}: {krate} (index {i}) depends on {dep}, but {dep} is \
                     listed later (index {j}) — cargo publish would fail resolving \
                     {dep} from crates.io before it's been published"
                );
            }
        }
    }
}
