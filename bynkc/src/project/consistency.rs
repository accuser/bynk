use super::*;

/// Within a multi-file unit (i.e., 2+ files in the same directory that share
/// a qualified name), every file must declare exactly the same name.
///
/// In v0.4 the same directory may contain multiple *single-file* units (one
/// commons and one context, say), provided each file's path matches the
/// last segment of its declared qualified name. Mixed-name files in one
/// directory are only flagged when they collide on the same name (handled by
/// [`check_group_kind_consistency`]) or when path/name alignment fails.
pub(crate) fn check_directory_name_consistency(
    parsed: &[ParsedFile],
) -> Result<(), Vec<CompileError>> {
    let mut errors: Vec<CompileError> = Vec::new();
    // For each unit (group of files sharing the same name), verify they all
    // live in the same directory. Tests are excluded — their files are
    // grouped by target, not by their own physical layout.
    let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, pf) in parsed.iter().enumerate() {
        if matches!(pf.kind, UnitKind::Test | UnitKind::Integration) {
            continue;
        }
        by_name.entry(pf.unit.name().joined()).or_default().push(i);
    }
    for indices in by_name.values() {
        if indices.len() < 2 {
            continue;
        }
        let first_dir = parsed[indices[0]]
            .source_path
            .parent()
            .unwrap_or(Path::new(""))
            .to_path_buf();
        for &idx in indices.iter().skip(1) {
            let dir = parsed[idx]
                .source_path
                .parent()
                .unwrap_or(Path::new(""))
                .to_path_buf();
            if dir != first_dir {
                errors.push(
                    CompileError::new(
                        "bynk.project.inconsistent_commons_name",
                        parsed[idx].unit.span(),
                        format!(
                            "files declaring `{}` are spread across different directories: `{}` vs `{}`",
                            parsed[idx].unit.name().joined(),
                            first_dir.display(),
                            dir.display(),
                        ),
                    )
                    .with_label(parsed[indices[0]].unit.span(), "first file is here")
                    .with_note(
                        "all files of a multi-file commons or context must live in the same directory",
                    ),
                );
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Within a multi-file unit (files sharing a qualified name), every file must
/// agree on kind. Handled by [`check_group_kind_consistency`]; this check is
/// the v0.4-style directory-level guard which now defers to it.
pub(crate) fn check_directory_kind_consistency(
    _parsed: &[ParsedFile],
) -> Result<(), Vec<CompileError>> {
    Ok(())
}

/// Each file's relative path must match its declared qualified name. Two
/// arrangements are valid:
/// - **Single-file**: `a/b/c.bynk` declaring `a.b.c`.
/// - **Multi-file**: `a/b/c/<any>.bynk` declaring `a.b.c`.
pub(crate) fn check_path_name_alignment(parsed: &[ParsedFile]) -> Result<(), Vec<CompileError>> {
    let mut errors: Vec<CompileError> = Vec::new();
    for pf in parsed {
        if matches!(pf.kind, UnitKind::Test | UnitKind::Integration) {
            // Test files are not required to match their target's path.
            continue;
        }
        let name = pf.unit.name().joined();
        let name_parts: Vec<&str> = name.split('.').collect();
        let rel = &pf.source_path;
        if !unit_path_matches(rel, &name) {
            errors.push(
                CompileError::new(
                    "bynk.project.inconsistent_commons_name",
                    pf.unit.span(),
                    format!(
                        "file `{}` declares `{name}`, but its path doesn't match — expected either `{}.bynk` (single-file) or `{}/...bynk` (multi-file)",
                        rel.display(),
                        name_parts.join("/"),
                        name_parts.join("/"),
                    ),
                )
                .with_note(
                    "the source-tree layout determines a unit's identity: each commons or context's qualified name must match its path",
                ),
            );
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// v0.9.1: in split-paths mode, a test file's path (relative to the
/// configured `tests` root) must match the test's declared **target**
/// qualified name. Same path-alignment logic as `check_path_name_alignment`,
/// but applied to test units.
pub(crate) fn check_test_path_alignment(parsed: &[ParsedFile]) -> Result<(), Vec<CompileError>> {
    let mut errors: Vec<CompileError> = Vec::new();
    for pf in parsed {
        if pf.kind != UnitKind::Test {
            continue;
        }
        let Some(test_decl) = pf.test() else { continue };
        let target_name = test_decl.target.joined();
        let target_parts: Vec<&str> = target_name.split('.').collect();
        let rel = &pf.source_path;
        // #47: accept the self-identifying `<target>.test.bynk` form too — a
        // test file at `<path>.test.bynk` aligns exactly as `<path>.bynk` does.
        if !unit_path_matches(&strip_test_infix(rel), &target_name) {
            let p = target_parts.join("/");
            errors.push(
                CompileError::new(
                    "bynk.project.inconsistent_test_path",
                    pf.unit.span(),
                    format!(
                        "test file `{}` targets `{target_name}`, but its path doesn't match — expected `{p}.bynk` / `{p}.test.bynk` (single-file) or `{p}/...bynk` (multi-file)",
                        rel.display(),
                    ),
                )
                .with_note(
                    "in split-paths mode (configured via `bynk.toml`'s `[paths]`), each test file's path under the `tests` directory must match its target's qualified name; a `.test.bynk` suffix is allowed",
                ),
            );
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Files grouped by qualified name must agree on kind (even across directories).
pub(crate) fn check_group_kind_consistency(
    parsed: &[ParsedFile],
    groups: &HashMap<String, Vec<usize>>,
) -> Result<(), Vec<CompileError>> {
    let mut errors: Vec<CompileError> = Vec::new();
    for (name, indices) in groups {
        if indices.len() < 2 {
            continue;
        }
        let first_kind = parsed[indices[0]].kind;
        for &idx in indices.iter().skip(1) {
            if parsed[idx].kind != first_kind {
                errors.push(
                    CompileError::new(
                        "bynk.project.kind_conflict",
                        parsed[idx].unit.span(),
                        format!(
                            "name `{name}` is declared as both a {} and a {}",
                            first_kind.display(),
                            parsed[idx].kind.display(),
                        ),
                    )
                    .with_label(
                        parsed[indices[0]].unit.span(),
                        format!("first declared as a {} here", first_kind.display()),
                    ),
                );
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}
