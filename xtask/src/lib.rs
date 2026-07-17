//! The pending-increment format validator (increment-allocation track, Slice 0).
//!
//! A feature PR adds one `design/pending/<slug>.md` declaring its bump level, a
//! one-line changelog blurb, and — when it records a decision — one or more ADR
//! prose blocks. It writes *no* version and *no* ADR number: those are the two
//! serial counters the track defers to a merge-time stamp (a later slice), so
//! that parallel increments stop conflicting on them. See
//! `design/tracks/increment-allocation.md` and `design/pending/README.md`.
//!
//! This module is the *format contract* between that human-authored file and the
//! future stamp. It is process tooling, not compiler behaviour, which is why it
//! lives in the unpublished `xtask` crate rather than in `bynkc`'s test suite.
//! [`check_all`] is exercised two ways: an integration test (`tests/pending_files.rs`)
//! runs it over the real `design/pending/**` as a drift guard, and the
//! `check-pending` binary subcommand exposes it for local runs.

use std::fs;
use std::path::{Path, PathBuf};

/// The bump level an increment declares. The stamp turns this into the next
/// `X.Y.Z` in merge order; the format never carries a concrete number.
#[derive(Debug, PartialEq, Eq)]
pub enum Level {
    Minor,
    Patch,
}

/// One ADR prose block. The stamp moves `body` verbatim into
/// `design/decisions/NNNN-<slug>.md`, assigning `NNNN` at merge.
#[derive(Debug, PartialEq, Eq)]
pub struct Adr {
    pub slug: String,
    pub body: String,
}

/// A parsed, validated pending-increment file.
#[derive(Debug, PartialEq, Eq)]
pub struct Pending {
    pub level: Level,
    pub changelog: String,
    pub adrs: Vec<Adr>,
}

/// `design/pending/`, resolved from this crate's manifest dir so the location is
/// independent of the working directory (the same trick `decisions_index` uses).
pub fn pending_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../design/pending")
}

/// Validate every `*.md` under [`pending_dir`] except `README.md` (the format
/// doc, excluded like the decisions index excludes its own README). Returns the
/// number of pending files validated, or every error found across all files
/// (each prefixed with its filename) so one run reports the whole picture.
pub fn check_all() -> Result<usize, Vec<String>> {
    let dir = pending_dir();
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(err) => return Err(vec![format!("cannot read {}: {err}", dir.display())]),
    };

    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".md") && n != "README.md")
        .collect();
    names.sort();

    let mut errors = Vec::new();
    for name in &names {
        let content = match fs::read_to_string(dir.join(name)) {
            Ok(c) => c,
            Err(err) => {
                errors.push(format!("{name}: cannot read: {err}"));
                continue;
            }
        };
        if let Err(errs) = validate(name, &content) {
            for e in errs {
                errors.push(format!("{name}: {e}"));
            }
        }
    }

    if errors.is_empty() {
        Ok(names.len())
    } else {
        Err(errors)
    }
}

/// Validate a single pending file's `content`. `filename` is used to check the
/// stem is a kebab-case slug. Returns every problem found (not just the first),
/// so a malformed file reports completely.
pub fn validate(filename: &str, content: &str) -> Result<Pending, Vec<String>> {
    let mut errors = Vec::new();

    let stem = Path::new(filename)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !is_kebab(&stem) {
        errors.push(format!(
            "filename stem {stem:?} is not a kebab-case slug (a-z, 0-9, single hyphens)"
        ));
    }

    let (level, changelog) = match parse_frontmatter(content, &mut errors) {
        Some(fm) => fm,
        None => return Err(errors),
    };
    let adrs = parse_adrs(content, &mut errors);

    if errors.is_empty() {
        Ok(Pending {
            level: level.expect("no errors implies a level"),
            changelog: changelog.expect("no errors implies a changelog"),
            adrs,
        })
    } else {
        Err(errors)
    }
}

/// Parse and validate the `---`-delimited header. Pushes errors; returns the
/// two fields when both are present and well-formed. Returns `None` only when
/// the frontmatter block itself is missing/unterminated (nothing to recover).
fn parse_frontmatter(
    content: &str,
    errors: &mut Vec<String>,
) -> Option<(Option<Level>, Option<String>)> {
    let mut lines = content.lines();
    if lines.next().map(str::trim_end) != Some("---") {
        errors.push("must open with a `---` frontmatter fence on line 1".into());
        return None;
    }

    let mut header = Vec::new();
    let mut closed = false;
    for line in lines {
        if line.trim_end() == "---" {
            closed = true;
            break;
        }
        header.push(line);
    }
    if !closed {
        errors.push("frontmatter is not closed with a `---` fence".into());
        return None;
    }

    let mut level = None;
    let mut changelog = None;
    // Track key presence separately from a valid value: a key that is present
    // but malformed reports its own error and must not also be "missing".
    let mut saw_level = false;
    let mut saw_changelog = false;
    for raw in header {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            errors.push(format!("frontmatter line is not `key: value`: {raw:?}"));
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "level" => {
                if saw_level {
                    errors.push("duplicate frontmatter key `level`".into());
                }
                saw_level = true;
                level = match value {
                    "minor" => Some(Level::Minor),
                    "patch" => Some(Level::Patch),
                    other => {
                        errors.push(format!("level must be `minor` or `patch`, got {other:?}"));
                        None
                    }
                };
            }
            "changelog" => {
                if saw_changelog {
                    errors.push("duplicate frontmatter key `changelog`".into());
                }
                saw_changelog = true;
                if value.is_empty() {
                    errors.push("changelog must not be empty".into());
                } else if looks_like_version_prefix(value) {
                    errors.push(format!(
                        "changelog must not start with a version number (the stamp adds it): {value:?}"
                    ));
                } else {
                    changelog = Some(value.to_string());
                }
            }
            other => errors.push(format!("unknown frontmatter key {other:?}")),
        }
    }

    if !saw_level {
        errors.push("frontmatter is missing `level`".into());
    }
    if !saw_changelog {
        errors.push("frontmatter is missing `changelog`".into());
    }

    Some((level, changelog))
}

/// Parse `## ADR: <slug>` blocks from the body (everything after the closing
/// frontmatter fence). Zero blocks is valid — an increment may record no
/// decision. Pushes errors for a non-kebab or duplicate slug, or an empty body.
fn parse_adrs(content: &str, errors: &mut Vec<String>) -> Vec<Adr> {
    // Body starts after the second `---` fence.
    let mut fences = 0;
    let mut body_lines = Vec::new();
    for line in content.lines() {
        if fences < 2 {
            if line.trim_end() == "---" {
                fences += 1;
            }
            continue;
        }
        body_lines.push(line);
    }

    // A `## ADR:` line inside a ``` code fence is prose (e.g. a pending file
    // documenting the format inline), not a block header. Mark each line's
    // header-ness up front, toggling on backtick-fence delimiters, so both the
    // outer scan and the body-collecting loop below agree on where blocks start.
    let mut in_fence = false;
    let is_header: Vec<bool> = body_lines
        .iter()
        .map(|line| {
            if line.trim_start().starts_with("```") {
                in_fence = !in_fence;
                false
            } else {
                !in_fence && adr_header_slug(line).is_some()
            }
        })
        .collect();

    let mut adrs: Vec<Adr> = Vec::new();
    let mut i = 0;
    while i < body_lines.len() {
        if is_header[i] {
            let slug = adr_header_slug(body_lines[i])
                .expect("is_header implies an ADR header")
                .trim()
                .to_string();
            i += 1;
            let mut body = Vec::new();
            while i < body_lines.len() && !is_header[i] {
                body.push(body_lines[i]);
                i += 1;
            }
            let body = body.join("\n").trim().to_string();

            if !is_kebab(&slug) {
                errors.push(format!(
                    "ADR slug {slug:?} is not a kebab-case slug (a-z, 0-9, single hyphens)"
                ));
            } else if adrs.iter().any(|a| a.slug == slug) {
                errors.push(format!("duplicate ADR slug {slug:?}"));
            }
            if body.is_empty() {
                errors.push(format!("ADR {slug:?} has an empty body"));
            }
            adrs.push(Adr { slug, body });
        } else {
            i += 1;
        }
    }
    adrs
}

/// The slug text of a `## ADR: <slug>` header line, if this line is one.
fn adr_header_slug(line: &str) -> Option<&str> {
    line.trim().strip_prefix("## ADR:")
}

/// A kebab-case slug: non-empty, `a-z0-9` and single interior hyphens only.
fn is_kebab(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('-')
        && !s.ends_with('-')
        && !s.contains("--")
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Whether the changelog's first token reads as a repo version the author has
/// accidentally prefixed — the stamp prepends the number, so the blurb must not
/// carry one. Matched to the repo's actual spellings: a `v` prefix (`v0.186`,
/// the banner form) or three-plus numeric groups (`0.186.0`, the Cargo form).
/// A bare two-group token like `3.0` is *not* a version here, so a blurb such as
/// "3.0 rendering pipeline added" is allowed.
fn looks_like_version_prefix(changelog: &str) -> bool {
    let raw = changelog.split_whitespace().next().unwrap_or("");
    let had_v = raw.starts_with('v') || raw.starts_with('V');
    let groups: Vec<&str> = raw.trim_start_matches(['v', 'V']).split('.').collect();
    let all_numeric = groups.len() >= 2
        && groups
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
    all_numeric && (had_v || groups.len() >= 3)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(name: &str, content: &str) -> Pending {
        validate(name, content).unwrap_or_else(|e| panic!("expected valid, got {e:?}"))
    }
    fn err(name: &str, content: &str) -> Vec<String> {
        validate(name, content).expect_err("expected invalid")
    }

    #[test]
    fn minimal_no_adr_is_valid() {
        let p = ok(
            "add-a-thing.md",
            "---\nlevel: minor\nchangelog: Add a thing to the language\n---\n",
        );
        assert_eq!(p.level, Level::Minor);
        assert_eq!(p.changelog, "Add a thing to the language");
        assert!(p.adrs.is_empty());
    }

    #[test]
    fn patch_level_is_valid() {
        assert_eq!(
            ok(
                "fix-a-thing.md",
                "---\nlevel: patch\nchangelog: Fix a non-language thing\n---\n"
            )
            .level,
            Level::Patch
        );
    }

    #[test]
    fn one_adr_parses_slug_and_body() {
        let p = ok(
            "unit-tier.md",
            "---\nlevel: minor\nchangelog: Drive a handler at the unit tier\n---\n\n## ADR: unit-tier-service-address\n\n**Decision.** A case addresses by surface.\n",
        );
        assert_eq!(p.adrs.len(), 1);
        assert_eq!(p.adrs[0].slug, "unit-tier-service-address");
        assert!(p.adrs[0].body.contains("addresses by surface"));
    }

    #[test]
    fn two_adrs_parse() {
        let p = ok(
            "two.md",
            "---\nlevel: minor\nchangelog: Two decisions\n---\n\n## ADR: first-one\nBody one.\n\n## ADR: second-one\nBody two.\n",
        );
        assert_eq!(p.adrs.len(), 2);
        assert_eq!(p.adrs[0].slug, "first-one");
        assert_eq!(p.adrs[1].slug, "second-one");
    }

    #[test]
    fn bad_level_rejected() {
        assert!(
            err("x.md", "---\nlevel: major\nchangelog: x\n---\n")
                .iter()
                .any(|e| e.contains("level must be"))
        );
    }

    #[test]
    fn missing_level_rejected() {
        assert!(
            err("x.md", "---\nchangelog: x\n---\n")
                .iter()
                .any(|e| e.contains("missing `level`"))
        );
    }

    #[test]
    fn missing_changelog_rejected() {
        assert!(
            err("x.md", "---\nlevel: minor\n---\n")
                .iter()
                .any(|e| e.contains("missing `changelog`"))
        );
    }

    #[test]
    fn empty_changelog_rejected() {
        assert!(
            err("x.md", "---\nlevel: minor\nchangelog:   \n---\n")
                .iter()
                .any(|e| e.contains("changelog"))
        );
    }

    #[test]
    fn version_prefixed_changelog_rejected() {
        for cl in ["v0.186 Add a thing", "0.186.0 Add a thing"] {
            let content = format!("---\nlevel: minor\nchangelog: {cl}\n---\n");
            assert!(
                err("x.md", &content)
                    .iter()
                    .any(|e| e.contains("version number")),
                "expected rejection for {cl:?}"
            );
        }
    }

    #[test]
    fn plain_changelog_with_a_dot_is_allowed() {
        // A blurb ending in a version-like word must not false-positive; only the
        // *first* token is checked.
        ok(
            "x.md",
            "---\nlevel: minor\nchangelog: Support semver ranges like 1.2.3\n---\n",
        );
    }

    #[test]
    fn bare_two_group_leading_number_is_allowed() {
        // `3.0` is not a repo version (no `v`, only two groups) — a blurb may
        // legitimately open with it.
        ok(
            "x.md",
            "---\nlevel: minor\nchangelog: 3.0 rendering pipeline added\n---\n",
        );
    }

    #[test]
    fn duplicate_frontmatter_key_rejected() {
        assert!(
            err(
                "x.md",
                "---\nlevel: minor\nlevel: patch\nchangelog: x\n---\n"
            )
            .iter()
            .any(|e| e.contains("duplicate frontmatter key `level`"))
        );
    }

    #[test]
    fn adr_header_inside_a_code_fence_is_not_a_block() {
        // A pending file documenting the format inline must not have its fenced
        // `## ADR:` example split off into a spurious block.
        let p = ok(
            "x.md",
            "---\nlevel: minor\nchangelog: Document the format\n---\n\n\
             Example:\n\n```markdown\n## ADR: not-a-real-block\nfenced prose\n```\n\n\
             ## ADR: the-real-one\nReal body.\n",
        );
        assert_eq!(p.adrs.len(), 1);
        assert_eq!(p.adrs[0].slug, "the-real-one");
    }

    #[test]
    fn no_frontmatter_rejected() {
        assert!(
            err("x.md", "just some text\n")
                .iter()
                .any(|e| e.contains("open with a `---`"))
        );
    }

    #[test]
    fn unclosed_frontmatter_rejected() {
        assert!(
            err("x.md", "---\nlevel: minor\nchangelog: x\n")
                .iter()
                .any(|e| e.contains("not closed"))
        );
    }

    #[test]
    fn unknown_key_rejected() {
        assert!(
            err("x.md", "---\nlevel: minor\nchangelog: x\nversion: 9\n---\n")
                .iter()
                .any(|e| e.contains("unknown frontmatter key"))
        );
    }

    #[test]
    fn non_kebab_adr_slug_rejected() {
        assert!(
            err(
                "x.md",
                "---\nlevel: minor\nchangelog: x\n---\n\n## ADR: Not_Kebab\nBody.\n"
            )
            .iter()
            .any(|e| e.contains("not a kebab-case slug"))
        );
    }

    #[test]
    fn duplicate_adr_slug_rejected() {
        assert!(err(
            "x.md",
            "---\nlevel: minor\nchangelog: x\n---\n\n## ADR: dup\nBody a.\n\n## ADR: dup\nBody b.\n"
        )
        .iter()
        .any(|e| e.contains("duplicate ADR slug")));
    }

    #[test]
    fn empty_adr_body_rejected() {
        assert!(
            err(
                "x.md",
                "---\nlevel: minor\nchangelog: x\n---\n\n## ADR: empty\n\n## ADR: next\nBody.\n"
            )
            .iter()
            .any(|e| e.contains("empty body"))
        );
    }

    #[test]
    fn non_kebab_filename_rejected() {
        assert!(
            err("Not_A_Slug.md", "---\nlevel: minor\nchangelog: x\n---\n")
                .iter()
                .any(|e| e.contains("filename stem"))
        );
    }
}
