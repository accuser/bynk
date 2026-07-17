//! The merge-time stamp (increment-allocation track, Slice 1).
//!
//! On merge, [`plan`] reads the pending files, assigns the next version(s) in
//! merge order and the next ADR number(s), and [`apply`] materialises them:
//! it runs `scripts/bump-version.sh`, inserts the changelog row(s), writes each
//! `design/decisions/NNNN-<slug>.md` and its index row, and deletes the consumed
//! pending files. Deleting what it consumes is what makes a re-run a no-op —
//! which is why version assignment and ADR materialisation are one atomic pass
//! (the entangled-by-delete finding, proposal #689 DECISION A).
//!
//! The version bump is injected (see [`apply`]) so the whole flow is testable on
//! a fixture tree without running the real, side-effect-heavy bump script.

use crate::{Adr, Level, validated_pending_in};
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

/// A semantic version `MAJOR.MINOR.PATCH`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    /// The next version for `level`: `minor` bumps MINOR and resets PATCH,
    /// `patch` bumps PATCH. (MAJOR is 0 pre-1.0 and never moved here.)
    pub fn next(self, level: &Level) -> Version {
        match level {
            Level::Minor => Version {
                minor: self.minor + 1,
                patch: 0,
                ..self
            },
            Level::Patch => Version {
                patch: self.patch + 1,
                ..self
            },
        }
    }

    /// Parse an `X.Y.Z` string.
    pub fn parse(s: &str) -> Option<Version> {
        let mut it = s.trim().split('.');
        let major = it.next()?.parse().ok()?;
        let minor = it.next()?.parse().ok()?;
        let patch = it.next()?.parse().ok()?;
        if it.next().is_some() {
            return None;
        }
        Some(Version {
            major,
            minor,
            patch,
        })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl Version {
    /// The ADR/index version string, following the corpus convention: a MINOR
    /// increment is `MAJOR.MINOR` (`0.186`), a PATCH increment keeps its patch
    /// (`0.185.1`, like the existing `v0.29.1`). Since [`next`](Version::next)
    /// resets PATCH to 0 for a minor bump, `patch == 0` is exactly the minor case.
    pub fn short(self) -> String {
        if self.patch == 0 {
            format!("{}.{}", self.major, self.minor)
        } else {
            self.to_string()
        }
    }
}

/// One pending increment stamped with the version it will ship as.
#[derive(Debug)]
pub struct Stamped {
    /// The pending file that produced this (relative name under `design/pending/`).
    pub file: String,
    pub version: Version,
    pub changelog: String,
    pub adrs: Vec<Adr>,
}

/// The full stamp plan for the pending files present.
#[derive(Debug)]
pub struct Plan {
    pub base_version: Version,
    pub increments: Vec<Stamped>,
    /// The next free ADR number, for materialisation.
    pub first_adr_number: u32,
}

impl Plan {
    /// The version the manifests end on — the last increment's, or the base if
    /// there is nothing to stamp.
    pub fn final_version(&self) -> Version {
        self.increments
            .last()
            .map(|s| s.version)
            .unwrap_or(self.base_version)
    }

    pub fn is_empty(&self) -> bool {
        self.increments.is_empty()
    }
}

/// Build the plan for `root`: read the workspace version and the pending files,
/// and assign each pending file (in filename order — a deterministic proxy for
/// merge order) its own next version. Returns validation errors verbatim.
pub fn plan(root: &Path) -> Result<Plan, Vec<String>> {
    let cargo = root.join("Cargo.toml");
    let cargo_src = fs::read_to_string(&cargo)
        .map_err(|e| vec![format!("cannot read {}: {e}", cargo.display())])?;
    let base_version = parse_workspace_version(&cargo_src)
        .ok_or_else(|| vec![format!("no `version = \"X.Y.Z\"` in {}", cargo.display())])?;

    let pending = validated_pending_in(&root.join("design/pending"))?;
    let first_adr_number = next_adr_number(root);

    let mut version = base_version;
    let mut increments = Vec::new();
    for (file, p) in pending {
        version = version.next(&p.level);
        increments.push(Stamped {
            file,
            version,
            changelog: p.changelog,
            adrs: p.adrs,
        });
    }

    Ok(Plan {
        base_version,
        increments,
        first_adr_number,
    })
}

/// Apply `plan` to `root`: write ADR files + index rows, insert changelog rows,
/// run `bump` for the final version, then delete the consumed pending files.
///
/// Order matters twice. `bump` runs *after* the ADR + changelog edits — the real
/// caller passes one that runs `scripts/bump-version.sh` — and the pending-file
/// deletes run *after* `bump` succeeds, so a failed bump leaves the pending files
/// intact and the run stays retryable.
/// `bump` is injected so tests exercise the whole flow on a fixture tree with a
/// stub that just rewrites the fixture's `Cargo.toml`.
pub fn apply(root: &Path, plan: &Plan, bump: impl Fn(Version) -> io::Result<()>) -> io::Result<()> {
    if plan.is_empty() {
        return Ok(());
    }

    let decisions = root.join("design/decisions");
    let changelog_path = root.join("site/src/content/docs/book/reference/changelog.md");

    // Materialise ADRs (ascending number) and collect the rows to prepend.
    let mut number = plan.first_adr_number;
    let mut changelog_rows = Vec::new();
    let mut index_rows = Vec::new();
    for inc in &plan.increments {
        for adr in &inc.adrs {
            let path = decisions.join(format!("{number:04}-{}.md", adr.slug));
            fs::write(&path, adr_file_contents(number, adr, inc.version))?;
            index_rows.push(index_row(number, adr, inc.version));
            number += 1;
        }
        changelog_rows.push(changelog_row(inc.version, &inc.changelog));
    }

    // Newest on top: the highest version/number is applied last, so reverse the
    // ascending lists before prepending them as a block.
    changelog_rows.reverse();
    index_rows.reverse();

    let changelog = fs::read_to_string(&changelog_path)?;
    let changelog = insert_after_table_separator(&changelog, "| Version |", &changelog_rows)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    fs::write(&changelog_path, changelog)?;

    if !index_rows.is_empty() {
        let readme_path = decisions.join("README.md");
        let readme = fs::read_to_string(&readme_path)?;
        let readme = insert_after_table_separator(&readme, "| # | Decision |", &index_rows)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        fs::write(&readme_path, readme)?;
    }

    // Bump the manifests to the final version.
    bump(plan.final_version())?;

    // Consume last: delete each pending file only once the bump has succeeded, so
    // a failed bump leaves the pending files intact and the run is retryable
    // (after discarding the partial changelog/ADR edits) rather than stranding a
    // changelog that cites a version the manifests never reached.
    for inc in &plan.increments {
        fs::remove_file(root.join("design/pending").join(&inc.file))?;
    }
    Ok(())
}

// --- Pure rendering / parsing helpers (unit-tested without the filesystem) ---

/// The `[workspace.package] version = "X.Y.Z"` from a `Cargo.toml`. Matches a
/// line-anchored `version = "..."` (the workspace version), not the inline
/// `version = "..."` inside a dependency spec — the same anchor `bump-version.sh`
/// rewrites.
pub fn parse_workspace_version(cargo_toml: &str) -> Option<Version> {
    cargo_toml.lines().find_map(|line| {
        let rest = line.strip_prefix("version = \"")?;
        let value = rest.strip_suffix('"')?;
        Version::parse(value)
    })
}

/// The next free ADR number: one past the highest `NNNN-*.md` in `design/decisions`.
pub fn next_adr_number(root: &Path) -> u32 {
    let dir = root.join("design/decisions");
    let max = fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let bytes = name.as_bytes();
            let is_adr = name.ends_with(".md") && bytes.len() > 5 && bytes[4] == b'-';
            (is_adr && bytes[..4].iter().all(u8::is_ascii_digit))
                .then(|| name[..4].parse::<u32>().ok())
                .flatten()
        })
        .max()
        .unwrap_or(0);
    max + 1
}

/// The changelog table row for a stamped increment.
pub fn changelog_row(version: Version, blurb: &str) -> String {
    format!("| **v{version}** | {blurb} |")
}

/// The `design/decisions/NNNN-<slug>.md` file contents: a `# NNNN — <title>`
/// heading, a status line carrying the version, then the body verbatim.
pub fn adr_file_contents(number: u32, adr: &Adr, version: Version) -> String {
    format!(
        "# {number:04} — {title}\n\n- **Status:** {status} (v{ver})\n\n{body}\n",
        title = adr.title,
        status = adr.status(),
        ver = version.short(),
        body = adr.body,
    )
}

/// The `design/decisions/README.md` index row for a materialised ADR.
pub fn index_row(number: u32, adr: &Adr, version: Version) -> String {
    format!(
        "| [{number:04}]({number:04}-{slug}.md) | **{title}** (v{ver}) — {summary} | {status} (v{ver}) |",
        slug = adr.slug,
        title = adr.title,
        ver = version.short(),
        summary = adr.summary(),
        status = adr.status(),
    )
}

/// Insert `rows` (already newest-first) immediately after the separator line of
/// the table whose header row contains `header_anchor` (e.g. `"| Version |"`).
/// Anchoring on the header — rather than the first separator in the file — keeps
/// the insert correct if another table (a legend, say) is ever added above.
pub fn insert_after_table_separator(
    md: &str,
    header_anchor: &str,
    rows: &[String],
) -> Result<String, String> {
    if rows.is_empty() {
        return Ok(md.to_string());
    }
    let mut out = Vec::new();
    let mut seen_header = false;
    let mut inserted = false;
    for line in md.lines() {
        out.push(line.to_string());
        if inserted {
            continue;
        }
        if !seen_header {
            seen_header = line.contains(header_anchor);
        } else if is_table_separator(line) {
            out.extend(rows.iter().cloned());
            inserted = true;
        }
    }
    if !inserted {
        return Err(format!(
            "no table with header {header_anchor:?} (and a `|---|` separator) to insert after"
        ));
    }
    // Preserve a trailing newline if the input had one.
    let mut joined = out.join("\n");
    if md.ends_with('\n') {
        joined.push('\n');
    }
    Ok(joined)
}

/// A Markdown table separator row — `|`, `-`, `:` and spaces only, with at least
/// one `-`.
fn is_table_separator(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('|') && t.contains('-') && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adr(slug: &str, title: &str) -> Adr {
        Adr {
            slug: slug.into(),
            title: title.into(),
            summary: None,
            status: None,
            body: "**Decision.** Do the thing.".into(),
        }
    }

    #[test]
    fn version_next() {
        let v = Version {
            major: 0,
            minor: 185,
            patch: 0,
        };
        assert_eq!(v.next(&Level::Minor).to_string(), "0.186.0");
        assert_eq!(v.next(&Level::Patch).to_string(), "0.185.1");
        let p = Version {
            major: 0,
            minor: 185,
            patch: 3,
        };
        assert_eq!(p.next(&Level::Minor).to_string(), "0.186.0");
    }

    #[test]
    fn parse_workspace_version_ignores_dep_specs() {
        let cargo = "[workspace.package]\nversion = \"0.185.0\"\n\n[workspace.dependencies]\nbynk = { path = \"bynk\", version = \"0.185.0\" }\n";
        assert_eq!(
            parse_workspace_version(cargo).unwrap().to_string(),
            "0.185.0"
        );
    }

    #[test]
    fn changelog_row_format() {
        let v = Version {
            major: 0,
            minor: 186,
            patch: 0,
        };
        assert_eq!(
            changelog_row(v, "Add a thing"),
            "| **v0.186.0** | Add a thing |"
        );
    }

    #[test]
    fn version_short_follows_the_corpus_convention() {
        // Minor (patch == 0) → MAJOR.MINOR; patch → full.
        assert_eq!(
            Version {
                major: 0,
                minor: 186,
                patch: 0
            }
            .short(),
            "0.186"
        );
        assert_eq!(
            Version {
                major: 0,
                minor: 185,
                patch: 1
            }
            .short(),
            "0.185.1"
        );
    }

    #[test]
    fn adr_file_and_index_row_use_the_short_version() {
        let v = Version {
            major: 0,
            minor: 186,
            patch: 0,
        };
        let a = adr("a-slug", "The title");
        let file = adr_file_contents(206, &a, v);
        assert!(file.starts_with("# 0206 — The title\n"));
        // MINOR-only, matching every existing ADR — not the full 0.186.0.
        assert!(file.contains("- **Status:** Accepted (v0.186)"));
        assert!(file.trim_end().ends_with("Do the thing."));

        let row = index_row(206, &a, v);
        assert_eq!(
            row,
            "| [0206](0206-a-slug.md) | **The title** (v0.186) — The title | Accepted (v0.186) |"
        );
    }

    #[test]
    fn patch_increment_index_row_keeps_the_patch() {
        let v = Version {
            major: 0,
            minor: 185,
            patch: 1,
        };
        let a = adr("s", "T");
        assert!(index_row(9, &a, v).contains("(v0.185.1)"));
    }

    #[test]
    fn index_row_uses_explicit_summary_and_status() {
        let v = Version {
            major: 0,
            minor: 1,
            patch: 0,
        };
        let a = Adr {
            slug: "s".into(),
            title: "T".into(),
            summary: Some("the distillation".into()),
            status: Some("Proposed".into()),
            body: "b".into(),
        };
        assert_eq!(
            index_row(7, &a, v),
            "| [0007](0007-s.md) | **T** (v0.1) — the distillation | Proposed (v0.1) |"
        );
    }

    #[test]
    fn insert_prepends_after_the_anchored_table_newest_first() {
        let md =
            "## Recent increments\n\n| Version | Highlights |\n|---|---|\n| **v0.185.0** | Old |\n";
        let out = insert_after_table_separator(
            md,
            "| Version |",
            &["| NEW1 |".into(), "| NEW2 |".into()],
        )
        .unwrap();
        let lines: Vec<&str> = out.lines().collect();
        let sep = lines.iter().position(|l| *l == "|---|---|").unwrap();
        assert_eq!(lines[sep + 1], "| NEW1 |");
        assert_eq!(lines[sep + 2], "| NEW2 |");
        assert_eq!(lines[sep + 3], "| **v0.185.0** | Old |");
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn insert_skips_an_earlier_unrelated_table() {
        // A legend table sits above the target; the anchor must route past it.
        let md = "| Legend | Meaning |\n|---|---|\n| x | y |\n\n\
                  | Version | Highlights |\n|---|---|\n| **v0.185.0** | Old |\n";
        let out = insert_after_table_separator(md, "| Version |", &["| NEW |".into()]).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        // Inserted below the *second* separator, not the legend's.
        let target = lines
            .iter()
            .position(|l| *l == "| **v0.185.0** | Old |")
            .unwrap();
        assert_eq!(lines[target - 1], "| NEW |");
        assert!(
            !out.contains("| x | y |\n| NEW |"),
            "must not touch the legend table"
        );
    }

    #[test]
    fn insert_without_the_anchored_table_errors() {
        assert!(
            insert_after_table_separator("no table here\n", "| Version |", &["| r |".into()])
                .is_err()
        );
    }
}
