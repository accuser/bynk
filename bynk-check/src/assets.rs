//! #1883 (static assets), slice 1a: the checker-side rules for `asset name =
//! "path"` declarations that need no disk — the path-safety rule, and the
//! composition of a unit's visible assets through `uses` ([DECISION G]).
//!
//! Whether a declared file exists, and its content hash, are slice 1b's: the
//! checker stays disk-free (`bynk-wasm` compiles it with no filesystem), so
//! both arrive through the file reader 1b threads in from the driver.

use std::collections::{BTreeMap, HashMap};

use bynk_syntax::ast::{AssetDecl, CommonsItem};
use bynk_syntax::error::CompileError;

use crate::project_model::UnitInfo;

/// Why `path` is not a safe asset path, or `None` when it is.
///
/// An asset is public by construction, and the path is joined onto the
/// project root to find the file, so a path that can name something outside
/// the project is rejected outright: an absolute path (`/etc/x`, `\x`,
/// `C:/x`), and any `..` segment. Both separators count, so a Windows-style
/// path is judged the same on every host. An empty path names no file.
pub fn unsafe_asset_path_reason(path: &str) -> Option<&'static str> {
    if path.is_empty() {
        return Some("the path is empty");
    }
    let bytes = path.as_bytes();
    let has_drive = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    if path.starts_with('/') || path.starts_with('\\') || has_drive {
        return Some("the path is absolute");
    }
    if path.split(['/', '\\']).any(|seg| seg == "..") {
        return Some("the path contains a `..` segment");
    }
    None
}

/// `bynk.asset.unsafe_path` for `a`, when [`unsafe_asset_path_reason`]
/// rejects its path. One constructor for the project pipeline
/// (`symbols::build_unit_table`) and the single-file compile
/// (`bynk-emit`'s `compile`), so the two report it identically.
pub fn unsafe_path_error(a: &AssetDecl) -> Option<CompileError> {
    let reason = unsafe_asset_path_reason(&a.path)?;
    Some(
        CompileError::new(
            "bynk.asset.unsafe_path",
            a.path_span,
            format!("asset `{}` has an unsafe path: {reason}", a.name.name),
        )
        .with_note(
            "an asset path is relative to the project root and must stay inside it; assets are public, so nothing outside the project may be named",
        ),
    )
}

/// `bynk.asset.not_yet_emitted` for `a`: slice 1a checks an asset but cannot
/// build one, so every build refuses it (the project pipeline's build-mode
/// gate in `bynk-emit`'s `run_checks`, and the single-file compile, which is
/// always a build). Slice 1b removes it.
pub fn not_yet_emitted_error(a: &AssetDecl) -> CompileError {
    CompileError::new(
        "bynk.asset.not_yet_emitted",
        a.span,
        format!(
            "asset `{}` checks, but this compiler cannot build it yet",
            a.name.name
        ),
    )
    .with_note(
        "`asset` declarations are checked today and built from the next release (#1883); remove the declaration to build",
    )
}

/// [`not_yet_emitted_error`] for every asset among `items` — the project
/// pipeline's build-mode gate. A helper here, not a loop at the call site, so
/// `bynk-emit`'s `project.rs` names no syntax-tree type (the `ast_importers`
/// probe).
pub fn not_yet_emitted_errors(items: &[CommonsItem]) -> Vec<CompileError> {
    items
        .iter()
        .filter_map(|item| match item {
            CommonsItem::Asset(a) => Some(not_yet_emitted_error(a)),
            _ => None,
        })
        .collect()
}

/// The single-file compile's asset diagnostics: each asset's path check, then
/// its refusal. A single-file compile is always a build and has no unit
/// table, so both are reported here, in declaration order.
pub fn single_file_asset_errors(items: &[CommonsItem]) -> Vec<CompileError> {
    let mut errors = Vec::new();
    for item in items {
        if let CommonsItem::Asset(a) = item {
            errors.extend(unsafe_path_error(a));
            errors.push(not_yet_emitted_error(a));
        }
    }
    errors
}

/// Every asset visible in unit `name`: its own, plus those of each commons it
/// `uses` (one level, the same mixin rule `compose_unit_symbols` applies to
/// types and functions). A local asset wins over an imported one of the same
/// name; two used commons declaring the same name is
/// `bynk.uses.name_conflict` (`phase_uses_name_conflicts`), so which of them
/// is kept here is never observable in a program that checks.
pub fn compose_unit_assets(
    name: &str,
    unit_info: &BTreeMap<String, UnitInfo>,
) -> HashMap<String, AssetDecl> {
    let Some(info) = unit_info.get(name) else {
        return HashMap::new();
    };
    let mut combined = info.table.assets.clone();
    for used in &info.uses {
        let Some(used_info) = unit_info.get(used) else {
            continue;
        };
        for (asset_name, decl) in &used_info.table.assets {
            combined
                .entry(asset_name.clone())
                .or_insert_with(|| decl.clone());
        }
    }
    combined
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::analyse;

    /// [DECISION G]: a commons owns its stylesheet, and a context that `uses`
    /// it reads `.url` without naming the asset itself — in a free function
    /// and in a handler body, which is checked against its own per-handler
    /// view of the unit (`context_checks`).
    #[test]
    fn a_used_commons_asset_is_a_value_of_type_asset_with_a_string_url() {
        let a = analyse(&[
            (
                "compat/page.bynk",
                "commons compat.page\n\n---\nThe board's stylesheet.\n---\nasset style = \"assets/board.css\"\n\nfn link(a: Asset) -> String {\n  a.url\n}\n\nfn sheet() -> String {\n  style.url\n}\n",
            ),
            (
                "compat/board.bynk",
                "context compat.board\n\nuses compat.page\n\nfn head() -> String {\n  link(style)\n}\n\nservice web from http {\n  on GET(\"/\") () -> Effect[HttpResult[String]] by Visitor {\n    Ok(style.url)\n  }\n}\n",
            ),
        ]);
        a.assert_clean();
        assert_eq!(a.type_at("compat/page.bynk", "style.url"), "String");
        assert_eq!(a.type_at("compat/page.bynk", "a.url"), "String");
        assert_eq!(a.type_at("compat/board.bynk", "link(style)"), "String");
        assert_eq!(a.type_at("compat/board.bynk", "style.url"), "String");
    }

    /// A local binding shadows an asset of the same name, as it shadows a
    /// function; `asset` stays an ordinary identifier outside its
    /// declaration position (a contextual keyword).
    #[test]
    fn a_local_shadows_an_asset_and_asset_is_still_an_identifier() {
        let a = analyse(&[(
            "c.bynk",
            "context c\n\nasset style = \"a.css\"\n\nfn f(style: Int) -> Int {\n  style + 1\n}\n\nfn g(asset: Int) -> Int {\n  asset\n}\n",
        )]);
        a.assert_clean();
        assert_eq!(a.type_at("c.bynk", "style + 1"), "Int");
    }

    /// A unit's own asset wins over a used commons' asset of the same name;
    /// only two *used* commons colliding is an error.
    #[test]
    fn a_local_asset_wins_over_a_used_one() {
        let a = analyse(&[
            ("lib/a.bynk", "commons lib.a\n\nasset style = \"a.css\"\n"),
            (
                "app/web.bynk",
                "context app.web\n\nuses lib.a\n\nasset style = \"b.css\"\n\nfn f() -> String {\n  style.url\n}\n",
            ),
        ]);
        a.assert_clean();
    }

    #[test]
    fn two_used_commons_declaring_one_asset_name_conflict() {
        let a = analyse(&[
            ("lib/a.bynk", "commons lib.a\n\nasset style = \"a.css\"\n"),
            ("lib/b.bynk", "commons lib.b\n\nasset style = \"b.css\"\n"),
            (
                "app/web.bynk",
                "context app.web\n\nuses lib.a\nuses lib.b\n\nfn f() -> String {\n  style.url\n}\n",
            ),
        ]);
        a.assert_reports("bynk.uses.name_conflict");
    }

    #[test]
    fn an_asset_has_only_a_url_field() {
        let a = analyse(&[(
            "c.bynk",
            "context c\n\nasset style = \"a.css\"\n\nfn f() -> String {\n  style.path\n}\n",
        )]);
        a.assert_reports("bynk.types.unknown_field");
    }

    /// Placement, duplicates, name conflicts, path safety, the boundary rule
    /// and the reserved type name, one project each.
    #[test]
    fn each_declaration_rule_reports_its_own_code() {
        for (src, code) in [
            (
                "adapter c\n\nbinding \"./c.js\"\n\nasset style = \"a.css\"\n",
                "bynk.asset.outside_context_or_commons",
            ),
            (
                "context c\n\nasset style = \"a.css\"\nasset style = \"b.css\"\n",
                "bynk.asset.duplicate",
            ),
            (
                "context c\n\nfn style() -> Int {\n  1\n}\n\nasset style = \"a.css\"\n",
                "bynk.resolve.name_conflict",
            ),
            (
                "context c\n\ntype Style = Int\n\nasset Style = \"a.css\"\n",
                "bynk.resolve.name_conflict",
            ),
            (
                "context c\n\nasset style = \"/etc/passwd\"\n",
                "bynk.asset.unsafe_path",
            ),
            (
                "context c\n\nasset style = \"../x.css\"\n",
                "bynk.asset.unsafe_path",
            ),
            (
                "context c\n\ntype Page = { style: Asset }\n",
                "bynk.types.asset_at_boundary",
            ),
            (
                "context c\n\ntype Asset = Int\n",
                "bynk.resolve.reserved_builtin_type",
            ),
        ] {
            let a = analyse(&[("c.bynk", src)]);
            a.assert_reports(code);
        }
    }

    #[test]
    fn unsafe_asset_path_reason_accepts_relative_paths_and_rejects_escapes() {
        for ok in [
            "assets/board.css",
            "board.css",
            "assets/.well-known/security.txt",
            "a..b/c.css",
            "./assets/x.css",
        ] {
            assert_eq!(unsafe_asset_path_reason(ok), None, "{ok} is safe");
        }
        for (bad, why) in [
            ("", "the path is empty"),
            ("/etc/passwd", "the path is absolute"),
            ("\\x.css", "the path is absolute"),
            ("C:/x.css", "the path is absolute"),
            ("../secret.txt", "the path contains a `..` segment"),
            ("assets/../../x", "the path contains a `..` segment"),
            ("assets\\..\\x", "the path contains a `..` segment"),
            ("assets/..", "the path contains a `..` segment"),
        ] {
            assert_eq!(unsafe_asset_path_reason(bad), Some(why), "{bad}");
        }
    }
}
