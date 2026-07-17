//! End-to-end `stamp` on a fixture tree — the whole apply flow (changelog rows,
//! ADR files + index rows, pending-file deletion, the injected version bump)
//! without running the real, side-effect-heavy `bump-version.sh`.

use std::fs;
use std::path::{Path, PathBuf};
use xtask::stamp::{self, Version};

/// A throwaway repo fixture. Named per the calling test so parallel runs don't
/// collide; removed and recreated on construction.
fn fixture(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("xtask-stamp-{tag}"));
    let _ = fs::remove_dir_all(&root);
    let write = |rel: &str, body: &str| {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
    };

    write("Cargo.toml", "[workspace.package]\nversion = \"0.185.0\"\n");
    write(
        "site/src/content/docs/book/reference/changelog.md",
        "# Changelog\n\n## Recent increments\n\n| Version | Highlights |\n|---|---|\n| **v0.185.0** | Old thing |\n",
    );
    write(
        "design/decisions/README.md",
        "# Decision records\n\n| # | Decision | Status |\n|---|---|---|\n| [0205](0205-old.md) | **Old** — old | Accepted (v0.185) |\n",
    );
    write("design/decisions/0205-old.md", "# 0205 — Old\n");
    fs::create_dir_all(root.join("design/pending")).unwrap();
    root
}

fn bumper(root: &Path) -> impl Fn(Version) -> std::io::Result<()> + '_ {
    move |v: Version| {
        let cargo = root.join("Cargo.toml");
        let src = fs::read_to_string(&cargo)?;
        let out = src
            .lines()
            .map(|l| {
                if l.starts_with("version = \"") {
                    format!("version = \"{v}\"")
                } else {
                    l.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&cargo, out + "\n")
    }
}

#[test]
fn stamp_one_increment_with_an_adr() {
    let root = fixture("one");
    fs::write(
        root.join("design/pending/feat-thing.md"),
        "---\nlevel: minor\nchangelog: Add the thing\n---\n\n## ADR: the-thing\ntitle: The thing is added\nsummary: How the thing works\n\n**Decision.** Add it.\n",
    )
    .unwrap();

    let plan = stamp::plan(&root).expect("plan");
    assert_eq!(
        plan.final_version(),
        Version {
            major: 0,
            minor: 186,
            patch: 0
        }
    );
    assert_eq!(plan.first_adr_number, 206);

    stamp::apply(&root, &plan, bumper(&root)).expect("apply");

    // Changelog: new row on top.
    let changelog =
        fs::read_to_string(root.join("site/src/content/docs/book/reference/changelog.md")).unwrap();
    let sep = changelog.find("|---|---|").unwrap();
    let after = &changelog[sep..];
    assert!(
        after.contains("| **v0.186.0** | Add the thing |"),
        "changelog row missing:\n{changelog}"
    );
    assert!(
        after.find("v0.186.0").unwrap() < after.find("v0.185.0").unwrap(),
        "new row must sit above the old one"
    );

    // ADR file 0206 written with title/status/body (MINOR-only version string).
    let adr = fs::read_to_string(root.join("design/decisions/0206-the-thing.md")).unwrap();
    assert!(adr.starts_with("# 0206 — The thing is added\n"));
    assert!(adr.contains("- **Status:** Accepted (v0.186)"));
    assert!(adr.contains("**Decision.** Add it."));

    // Index row for 0206, using the explicit summary.
    let readme = fs::read_to_string(root.join("design/decisions/README.md")).unwrap();
    assert!(readme.contains(
        "| [0206](0206-the-thing.md) | **The thing is added** (v0.186) — How the thing works | Accepted (v0.186) |"
    ));

    // Pending file consumed; manifests bumped.
    assert!(!root.join("design/pending/feat-thing.md").exists());
    assert!(
        fs::read_to_string(root.join("Cargo.toml"))
            .unwrap()
            .contains("version = \"0.186.0\"")
    );

    // Idempotent: a re-run finds nothing to do.
    let replan = stamp::plan(&root).expect("replan");
    assert!(replan.is_empty());
}

#[test]
fn stamp_two_increments_get_sequential_versions() {
    let root = fixture("two");
    fs::write(
        root.join("design/pending/a-first.md"),
        "---\nlevel: minor\nchangelog: First\n---\n",
    )
    .unwrap();
    fs::write(
        root.join("design/pending/b-second.md"),
        "---\nlevel: patch\nchangelog: Second\n---\n",
    )
    .unwrap();

    let plan = stamp::plan(&root).expect("plan");
    // Filename order: a-first (minor -> 0.186.0), then b-second (patch -> 0.186.1).
    assert_eq!(
        plan.increments
            .iter()
            .map(|s| s.version.to_string())
            .collect::<Vec<_>>(),
        vec!["0.186.0", "0.186.1"]
    );
    assert_eq!(
        plan.final_version(),
        Version {
            major: 0,
            minor: 186,
            patch: 1
        }
    );

    stamp::apply(&root, &plan, bumper(&root)).expect("apply");

    let changelog =
        fs::read_to_string(root.join("site/src/content/docs/book/reference/changelog.md")).unwrap();
    // Newest (0.186.1) on top, then 0.186.0, then the pre-existing 0.185.0.
    let pos = |v: &str| changelog.find(v).unwrap();
    assert!(pos("v0.186.1") < pos("v0.186.0"));
    assert!(pos("v0.186.0") < pos("v0.185.0"));
    assert!(
        fs::read_to_string(root.join("Cargo.toml"))
            .unwrap()
            .contains("version = \"0.186.1\"")
    );
}

#[test]
fn two_adrs_across_two_increments_number_sequentially() {
    let root = fixture("twoadr");
    fs::write(
        root.join("design/pending/a-first.md"),
        "---\nlevel: minor\nchangelog: First\n---\n\n## ADR: alpha\ntitle: Alpha\n\nBody a.\n",
    )
    .unwrap();
    fs::write(
        root.join("design/pending/b-second.md"),
        "---\nlevel: patch\nchangelog: Second\n---\n\n## ADR: beta\ntitle: Beta\n\nBody b.\n",
    )
    .unwrap();

    let plan = stamp::plan(&root).expect("plan");
    assert_eq!(plan.first_adr_number, 206);
    stamp::apply(&root, &plan, bumper(&root)).expect("apply");

    // First increment's ADR is 0206 at its version (0.186); second is 0207 at 0.186.1.
    let alpha = fs::read_to_string(root.join("design/decisions/0206-alpha.md")).unwrap();
    assert!(alpha.starts_with("# 0206 — Alpha\n"));
    assert!(alpha.contains("(v0.186)"));
    let beta = fs::read_to_string(root.join("design/decisions/0207-beta.md")).unwrap();
    assert!(beta.starts_with("# 0207 — Beta\n"));
    assert!(beta.contains("(v0.186.1)"));

    // Both index rows present; newest (0207) above 0206, both above the old 0205.
    let readme = fs::read_to_string(root.join("design/decisions/README.md")).unwrap();
    let pos = |s: &str| readme.find(s).unwrap();
    assert!(pos("0207-beta.md") < pos("0206-alpha.md"));
    assert!(pos("0206-alpha.md") < pos("0205-old.md"));
}

#[test]
fn empty_pending_is_a_noop() {
    let root = fixture("empty");
    fs::create_dir_all(root.join("design/pending")).unwrap();
    let plan = stamp::plan(&root).expect("plan");
    assert!(plan.is_empty());
    // apply must not touch anything or fail.
    stamp::apply(&root, &plan, |_| {
        panic!("bump must not run for an empty plan")
    })
    .expect("apply");
    assert!(
        fs::read_to_string(root.join("Cargo.toml"))
            .unwrap()
            .contains("version = \"0.185.0\"")
    );
}
