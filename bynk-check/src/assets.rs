//! #1883 (static assets): the checker-side rules for `asset name = "path"`
//! declarations — the path rules, the name rule, the composition of a unit's
//! visible assets through `uses` ([DECISION G]) — and (slice 1b) the build
//! side's shared definitions: the existence check, the served URL path, and
//! the files each Worker ships.
//!
//! The checker stays disk-free (`bynk-wasm` compiles it with no filesystem):
//! the project's `assets/` folder reaches it pre-read by the caller
//! (`bynk_project::assets`), as a path set or as bytes.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use bynk_syntax::ast::{AssetDecl, CommonsItem, FnName, Ident};
use bynk_syntax::error::CompileError;

use crate::project_model::{ErrorSink, UnitInfo};
use bynk_project::ParsedFile;

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

/// `bynk.asset.outside_assets_dir` for `a`, when its path is safe but does
/// not lie inside the project's assets folder
/// ([`bynk_project::assets::PROJECT_ASSETS_DIR`]). Assets are public, so they
/// come only from that folder, and a declaration can never publish `.env` or
/// a source file by mistake (#1883, slice 1b).
pub fn outside_assets_dir_error(a: &AssetDecl) -> Option<CompileError> {
    if unsafe_asset_path_reason(&a.path).is_some()
        || bynk_project::assets::is_in_assets_dir(&a.path)
    {
        return None;
    }
    let dir = bynk_project::assets::PROJECT_ASSETS_DIR;
    Some(
        CompileError::new(
            "bynk.asset.outside_assets_dir",
            a.path_span,
            format!(
                "asset `{}` must name a file in the project's `{dir}/` folder",
                a.name.name
            ),
        )
        .with_note(format!(
            "assets are public, so they come only from `{dir}/`; move the file there and name it `{dir}/…`"
        )),
    )
}

/// The path rules for `a`, in order: [`unsafe_path_error`], then
/// [`outside_assets_dir_error`] (at most one of the two applies).
pub fn path_errors(a: &AssetDecl) -> Option<CompileError> {
    unsafe_path_error(a).or_else(|| outside_assets_dir_error(a))
}

/// The URL path prefix every asset is served under. A handler route under it
/// is reserved for assets (slice 2's check), and `_headers` gives everything
/// under it an immutable cache lifetime.
pub const SERVED_PREFIX: &str = "/_assets/";

/// `bynk.asset.not_served` (a warning) for each asset among `items`, for a
/// deployable Bundle build: a Bundle ships no Worker and so no files, yet each
/// module still carries its asset's `.url`, which would 404 wherever the
/// output is served. A warning, not an error, because a Bundle build is also
/// how `bynkc test` compiles; the caller raises it only for a build without
/// tests (review of #1904). A helper here keeps `bynk-emit`'s `project.rs`
/// free of syntax-tree types (the `ast_importers` probe).
pub fn bundle_not_served_warnings(items: &[CommonsItem]) -> Vec<CompileError> {
    items
        .iter()
        .filter_map(|item| match item {
            CommonsItem::Asset(a) => Some(
                CompileError::new(
                    "bynk.asset.not_served",
                    a.span,
                    format!(
                        "asset `{}` is not served by a Bundle build: its `.url` names a file this build does not ship",
                        a.name.name
                    ),
                )
                .with_note(
                    "assets are served beside a Worker; build with `--target workers` (as `bynk dev` and `bynk deploy` do)",
                ),
            ),
            _ => None,
        })
        .collect()
}

/// The URL path the platform serves the asset file `source` (a normalised
/// `assets/…` key) with content `bytes` at:
/// `/_assets/<stem>.<hash>.<ext>`, where `<hash>` is the content's FNV-1a 64
/// ([`crate::contract::fnv1a64_hex`]) and `<stem>`/`<ext>` are the file
/// name's, split at its last `.` (no `.<ext>` when it has none).
///
/// Only the file name is kept, not its folders: the hash already tells two
/// files apart, and the same bytes under the same name are one served file,
/// however many declarations name them. Any character outside
/// `[A-Za-z0-9._-]` becomes `-`, so `.url` never needs percent-encoding.
/// Reproducible: it depends only on the name and the bytes.
pub fn served_path(source: &str, bytes: &[u8]) -> String {
    let name = source.rsplit('/').next().unwrap_or(source);
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let hash = crate::contract::fnv1a64_hex(bytes);
    match clean.rfind('.') {
        Some(dot) if dot > 0 => {
            format!(
                "{SERVED_PREFIX}{}.{hash}.{}",
                &clean[..dot],
                &clean[dot + 1..]
            )
        }
        _ => format!("{SERVED_PREFIX}{clean}.{hash}"),
    }
}

/// Every declared asset's served path, keyed by its normalised source path —
/// what the emitter writes as each `.url`. Only declarations whose file is in
/// `files` appear (a missing one is `bynk.asset.missing_file`).
pub fn declared_served_paths(
    parsed: &[ParsedFile],
    files: &BTreeMap<String, Vec<u8>>,
) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for pf in parsed {
        for item in pf.items() {
            let CommonsItem::Asset(a) = item else {
                continue;
            };
            let key = bynk_project::assets::normalise_asset_path(&a.path);
            if let Some(bytes) = files.get(&key) {
                let served = served_path(&key, bytes);
                out.insert(key, served);
            }
        }
    }
    out
}

/// The files a context's Worker ships: the assets declared by the context and
/// by every commons in its **transitive** `uses` closure, keyed by served
/// path, each mapped to its normalised source path.
///
/// Transitive, unlike name visibility ([`compose_unit_assets`], one level):
/// commons `a` may use commons `b` and return `b`'s `.url`, and the Worker
/// running `a`'s code must still serve `b`'s file. Sorted and deduplicated by
/// served path, so the output is reproducible.
pub fn deployed_assets(
    context: &str,
    unit_uses: &HashMap<String, Vec<String>>,
    unit_tables: &HashMap<String, crate::symbols::UnitTable>,
    served: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut stack: Vec<&str> = vec![context];
    let mut out = BTreeMap::new();
    while let Some(unit) = stack.pop() {
        if !seen.insert(unit) {
            continue;
        }
        if let Some(table) = unit_tables.get(unit) {
            for decl in table.assets.values() {
                let key = bynk_project::assets::normalise_asset_path(&decl.path);
                if let Some(path) = served.get(&key) {
                    out.insert(path.clone(), key);
                }
            }
        }
        for used in unit_uses.get(unit).into_iter().flatten() {
            stack.push(used);
        }
    }
    out
}

/// `bynk.asset.missing_file` for each asset whose (otherwise valid) path names
/// no file in `available` — the project's `assets/` folder as the caller read
/// it (`bynk_project::assets`). `None` skips the check: nothing on disk was
/// consulted (an in-memory check). A build passes `Some`, an empty set when it
/// had no filesystem, so it never invents a URL for a file nobody read.
///
/// A path that already fails [`path_errors`] is not also reported missing.
pub fn phase_asset_files(
    parsed: &[ParsedFile],
    available: Option<&BTreeSet<String>>,
    errors: &mut ErrorSink,
) {
    let Some(available) = available else { return };
    for pf in parsed {
        for item in pf.items() {
            let CommonsItem::Asset(a) = item else {
                continue;
            };
            if path_errors(a).is_some() {
                continue;
            }
            let key = bynk_project::assets::normalise_asset_path(&a.path);
            if !available.contains(&key) {
                errors.push_for(
                    Some(&pf.identity_path()),
                    CompileError::new(
                        "bynk.asset.missing_file",
                        a.path_span,
                        format!("asset `{}` names `{key}`, which does not exist", a.name.name),
                    )
                    .with_note(
                        "an asset must be a regular file in the project's `assets/` folder; symbolic links are not followed",
                    ),
                );
            }
        }
    }
}

/// `bynk.asset.needs_project` for `a`: a single-file build emits one module
/// and no Worker, and has no project root to find `assets/` under, so it
/// cannot ship a file. Checking a single file (`bynk check page.bynk`) is
/// unaffected; building one that declares an asset is refused. Build the
/// project instead.
pub fn needs_project_error(a: &AssetDecl) -> CompileError {
    CompileError::new(
        "bynk.asset.needs_project",
        a.span,
        format!(
            "asset `{}` cannot be built from a single file",
            a.name.name
        ),
    )
    .with_note(
        "an asset is served beside a Worker, so it needs a project build (`bynk dev`, `bynkc compile <project dir>`) with the file in the project's `assets/` folder",
    )
}

/// The name rule for asset `a`, given what its unit already declares under
/// that name: an earlier asset is `bynk.asset.duplicate`; a free function or a
/// type is `bynk.resolve.name_conflict` (an asset is a value, in the
/// function namespace, and a type name in value position is an error). `None`
/// when the name is free. One rule for the project pipeline
/// (`symbols::build_unit_table`) and the single-file compile, which has no
/// unit table (review of #1903).
pub fn name_error(
    a: &AssetDecl,
    prev_asset: Option<&Ident>,
    prev_fn: Option<&Ident>,
    prev_type: Option<&Ident>,
) -> Option<CompileError> {
    let name = &a.name.name;
    if let Some(prev) = prev_asset {
        Some(
            CompileError::new(
                "bynk.asset.duplicate",
                a.name.span,
                format!("asset `{name}` is already declared"),
            )
            .with_label(prev.span, "previously declared here"),
        )
    } else if let Some(prev) = prev_fn {
        Some(
            CompileError::new(
                "bynk.resolve.name_conflict",
                a.name.span,
                format!("asset `{name}` conflicts with a function of the same name"),
            )
            .with_label(prev.span, "function declared here"),
        )
    } else {
        prev_type.map(|prev| {
            CompileError::new(
                "bynk.resolve.name_conflict",
                a.name.span,
                format!("asset `{name}` conflicts with a type of the same name"),
            )
            .with_label(prev.span, "type declared here")
        })
    }
}

/// The single-file compile's asset diagnostics, in declaration order: each
/// asset's path check and name rule ([`name_error`], against the file's own
/// functions, types and earlier assets), and, when `build` is set, its
/// refusal ([`needs_project_error`]). Placement needs no check here: a
/// single file is always a commons (an `adapter` is a parse error).
pub fn single_file_asset_errors(items: &[CommonsItem], build: bool) -> Vec<CompileError> {
    let mut fns: HashMap<&str, &Ident> = HashMap::new();
    let mut types: HashMap<&str, &Ident> = HashMap::new();
    for item in items {
        match item {
            CommonsItem::Fn(f) => {
                if let FnName::Free(id) = &f.name {
                    fns.entry(id.name.as_str()).or_insert(id);
                }
            }
            CommonsItem::Type(t) => {
                types.entry(t.name.name.as_str()).or_insert(&t.name);
            }
            CommonsItem::Event(e) => {
                types.entry(e.name.name.as_str()).or_insert(&e.name);
            }
            _ => {}
        }
    }
    let mut assets: HashMap<&str, &Ident> = HashMap::new();
    let mut errors = Vec::new();
    for item in items {
        let CommonsItem::Asset(a) = item else {
            continue;
        };
        errors.extend(path_errors(a));
        let key = a.name.name.as_str();
        match name_error(
            a,
            assets.get(key).copied(),
            fns.get(key).copied(),
            types.get(key).copied(),
        ) {
            Some(err) => errors.push(err),
            None => {
                assets.insert(key, &a.name);
            }
        }
        if build {
            errors.push(needs_project_error(a));
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

    /// The asset files these tests' declarations name, written into each
    /// project beside its sources so the existence check passes (it reads
    /// the `assets/` folder from disk, as the editor does).
    const ASSET_FILES: &[(&str, &str)] = &[
        ("assets/a.css", "a{}"),
        ("assets/b.css", "b{}"),
        ("assets/board.css", "body{}"),
    ];

    fn analyse(files: &[(&str, &str)]) -> crate::testkit::Analysed {
        let mut all = files.to_vec();
        all.extend_from_slice(ASSET_FILES);
        crate::testkit::analyse(&all)
    }

    /// #1883, slice 1b: a declared file must exist in `assets/`; a symlink
    /// there does not count.
    #[test]
    fn a_declared_file_must_exist_in_the_assets_folder() {
        let a = crate::testkit::analyse(&[(
            "c.bynk",
            "context c\n\nasset style = \"assets/missing.css\"\n",
        )]);
        a.assert_reports("bynk.asset.missing_file");

        let a = analyse(&[(
            "c.bynk",
            "context c\n\nasset style = \"./assets/board.css\"\n",
        )]);
        a.assert_clean();
    }

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
            "context c\n\nasset style = \"assets/a.css\"\n\nfn f(style: Int) -> Int {\n  style + 1\n}\n\nfn g(asset: Int) -> Int {\n  asset\n}\n",
        )]);
        a.assert_clean();
        assert_eq!(a.type_at("c.bynk", "style + 1"), "Int");
    }

    /// A unit's own asset wins over a used commons' asset of the same name;
    /// only two *used* commons colliding is an error.
    #[test]
    fn a_local_asset_wins_over_a_used_one() {
        let a = analyse(&[
            (
                "lib/a.bynk",
                "commons lib.a\n\nasset style = \"assets/a.css\"\n",
            ),
            (
                "app/web.bynk",
                "context app.web\n\nuses lib.a\n\nasset style = \"assets/b.css\"\n\nfn f() -> String {\n  style.url\n}\n",
            ),
        ]);
        a.assert_clean();
    }

    #[test]
    fn two_used_commons_declaring_one_asset_name_conflict() {
        let a = analyse(&[
            (
                "lib/a.bynk",
                "commons lib.a\n\nasset style = \"assets/a.css\"\n",
            ),
            (
                "lib/b.bynk",
                "commons lib.b\n\nasset style = \"assets/b.css\"\n",
            ),
            (
                "app/web.bynk",
                "context app.web\n\nuses lib.a\nuses lib.b\n\nfn f() -> String {\n  style.url\n}\n",
            ),
        ]);
        a.assert_reports("bynk.uses.name_conflict");
    }

    /// A unit split across files has one asset namespace: a second
    /// declaration in a sibling file is a duplicate, reported once, and a
    /// sibling's asset is visible without any `uses`.
    #[test]
    fn a_multi_file_unit_shares_one_asset_namespace() {
        let dup = analyse(&[
            ("c/a.bynk", "context c\n\nasset style = \"assets/a.css\"\n"),
            ("c/b.bynk", "context c\n\nasset style = \"assets/b.css\"\n"),
        ]);
        assert_eq!(
            dup.categories()
                .iter()
                .filter(|c| **c == "bynk.asset.duplicate")
                .count(),
            1,
            "{}",
            dup.render()
        );
        let shared = analyse(&[
            ("c/a.bynk", "context c\n\nasset style = \"assets/a.css\"\n"),
            (
                "c/b.bynk",
                "context c\n\nfn f() -> String {\n  style.url\n}\n",
            ),
        ]);
        shared.assert_clean();
    }

    /// Review of #1903: a local asset must not hide a real conflict between
    /// two used commons over a *type* (or function) name. An asset only wins
    /// in value position; a type position still resolves through the
    /// imported types, so the ambiguity remains and must be reported.
    #[test]
    fn a_local_asset_does_not_hide_a_two_import_type_conflict() {
        let a = analyse(&[
            ("lib/a.bynk", "commons lib.a\n\ntype Style = Int\n"),
            ("lib/b.bynk", "commons lib.b\n\ntype Style = String\n"),
            (
                "app/web.bynk",
                "context app.web\n\nuses lib.a\nuses lib.b\n\nasset Style = \"assets/a.css\"\n\ntype Page = { s: Style }\n",
            ),
        ]);
        a.assert_reports("bynk.uses.name_conflict");
    }

    #[test]
    fn an_asset_has_only_a_url_field() {
        let a = analyse(&[(
            "c.bynk",
            "context c\n\nasset style = \"assets/a.css\"\n\nfn f() -> String {\n  style.path\n}\n",
        )]);
        a.assert_reports("bynk.types.unknown_field");
    }

    /// Placement, duplicates, name conflicts, path safety, the boundary rule
    /// and the reserved type name, one project each.
    #[test]
    fn each_declaration_rule_reports_its_own_code() {
        for (src, code) in [
            (
                "adapter c\n\nbinding \"./c.js\"\n\nasset style = \"assets/a.css\"\n",
                "bynk.asset.outside_context_or_commons",
            ),
            (
                "context c\n\nasset style = \"assets/a.css\"\nasset style = \"assets/b.css\"\n",
                "bynk.asset.duplicate",
            ),
            (
                "context c\n\nfn style() -> Int {\n  1\n}\n\nasset style = \"assets/a.css\"\n",
                "bynk.resolve.name_conflict",
            ),
            (
                "context c\n\ntype Style = Int\n\nasset Style = \"assets/a.css\"\n",
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
            (
                "context c\n\nasset env = \".env\"\n",
                "bynk.asset.outside_assets_dir",
            ),
            (
                "context c\n\nasset page = \"src/c.bynk\"\n",
                "bynk.asset.outside_assets_dir",
            ),
            (
                "context c\n\nagent Board {\n  key id: String\n\n  store style: Cell[Asset]\n\n  on call get() -> Effect[Int] {\n    Effect.pure(1)\n  }\n}\n",
                "bynk.types.asset_at_boundary",
            ),
        ] {
            let a = analyse(&[("c.bynk", src)]);
            a.assert_reports(code);
        }
    }

    #[test]
    fn served_path_is_hashed_flat_and_url_safe() {
        let h = crate::contract::fnv1a64_hex(b"body{}");
        assert_eq!(
            served_path("assets/board.css", b"body{}"),
            format!("/_assets/board.{h}.css")
        );
        assert_eq!(
            served_path("assets/css/board.min.css", b"body{}"),
            format!("/_assets/board.min.{h}.css")
        );
        assert_eq!(
            served_path("assets/LICENSE", b"body{}"),
            format!("/_assets/LICENSE.{h}")
        );
        assert_eq!(
            served_path("assets/.nojekyll", b"body{}"),
            format!("/_assets/.nojekyll.{h}")
        );
        assert_eq!(
            served_path("assets/my file é.css", b"body{}"),
            format!("/_assets/my-file--.{h}.css")
        );
        assert_ne!(
            served_path("assets/board.css", b"a"),
            served_path("assets/board.css", b"b"),
            "a content change changes the URL"
        );
    }

    /// A Worker ships the files of its whole `uses` closure, not just the
    /// first level: `ctx -> a -> b -> c`, and only `c` declares an asset.
    #[test]
    fn deployed_assets_follow_the_transitive_uses_closure() {
        fn decl(name: &str, path: &str) -> AssetDecl {
            AssetDecl {
                name: Ident {
                    name: name.to_string(),
                    span: Default::default(),
                },
                path: path.to_string(),
                path_span: Default::default(),
                documentation: None,
                span: Default::default(),
                trivia: Default::default(),
            }
        }
        let mut tables: HashMap<String, crate::symbols::UnitTable> = HashMap::new();
        for unit in ["ctx", "a", "b", "c", "unused"] {
            tables.insert(unit.to_string(), Default::default());
        }
        tables
            .get_mut("c")
            .unwrap()
            .assets
            .insert("style".into(), decl("style", "./assets/board.css"));
        tables
            .get_mut("unused")
            .unwrap()
            .assets
            .insert("logo".into(), decl("logo", "assets/logo.png"));
        let uses: HashMap<String, Vec<String>> =
            [("ctx", vec!["a"]), ("a", vec!["b"]), ("b", vec!["c", "a"])]
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.into_iter().map(String::from).collect()))
                .collect();
        let served: BTreeMap<String, String> = [
            (
                "assets/board.css".to_string(),
                "/_assets/board.1.css".to_string(),
            ),
            (
                "assets/logo.png".to_string(),
                "/_assets/logo.2.png".to_string(),
            ),
        ]
        .into_iter()
        .collect();
        let deployed = deployed_assets("ctx", &uses, &tables, &served);
        assert_eq!(
            deployed.into_iter().collect::<Vec<_>>(),
            vec![(
                "/_assets/board.1.css".to_string(),
                "assets/board.css".to_string()
            )]
        );
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
