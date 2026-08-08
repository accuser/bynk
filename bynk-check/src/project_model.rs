//! Project-wide orchestration: discovery → parse → group → resolve, shared
//! between `bynk-emit`'s `run_checks` (both `Mode::Build` and `Mode::Analyse`)
//! and this crate's own [`crate::analysis::analyse_project`].
//!
//! P4.1 (#1115), second scope finding on the tracking issue: this pipeline —
//! `phase_discovery` through `assemble_unit_info`, plus the per-unit symbol
//! composition (`compose_unit_symbols`/`merge_consumed_exports`/
//! `collect_unit_methods`) — used to live only in `bynk-emit/src/project.rs`,
//! inline in `run_checks`. A literal no-indirection `bynk-check`-side analysis
//! entry point needs the identical sequence, so rather than write a second,
//! independently-maintained copy (the mistake this whole design track's
//! `extract, don't duplicate` principle exists to prevent — see
//! `lower_field_default_wire`, `build_capability_op_info` for the same move
//! made earlier in this track), it moved here. `bynk-emit`'s `run_checks`
//! becomes a caller of these functions instead of owning the logic, the same
//! way P4.0 turned `project.rs` into a caller of `bynk-project`.
//!
//! What stayed in `bynk-emit` (not shared, because only the `Mode::Build` path
//! needs it, or because it's genuinely emission-shaped): the whole-project
//! `messages`/locale-ambiguity/event-subscription checks, schema-registry
//! reconciliation, platform-lock, the `Mode::Build` bail gate, and everything
//! from emission onward (`EmitUnitCtx`, `emit_unit`,
//! `collect_history_target_agents`). `check_function_type_boundaries` also
//! stays in `bynk-emit` (`validate.rs`) — [`phase_group`] reaches it only
//! through an optional hook, never a direct call, so the new entry point can
//! genuinely omit it (a documented residual gap, see `analysis.rs`) rather
//! than duplicate it with a different diagnostic order.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use crate::checker::{self, Ty, TyId, Types};
use crate::context_checks::{build_capability_op_info, ts_type_ref_display};
use crate::firstparty::{self, Platform};
use crate::icu;
use crate::index::{RefSink, SymbolKind};
use crate::resolver::MethodTable as ResolverMethodTable;
use crate::symbols::{
    ConsumedType, ContextMessageBundle, FileDeclIndex, UnitTable, build_file_decl_index,
    build_unit_table, consumes_span_of, detect_context_message_bundle, parsed_alias_span,
    uses_span_of,
};
use bynk_project::{
    AttributedError, ParsedFile, UnitKind, check_directory_kind_consistency,
    check_directory_name_consistency, check_file_directory_conflicts, check_group_kind_consistency,
    check_path_name_alignment, detect_consumes_cycles, discover_bynk_files, is_unpinned_range,
    normalize_rel, parse_sources, read_adapter_binding, read_source,
};
use bynk_syntax::ast::*;
use bynk_syntax::error::CompileError;
use bynk_syntax::lexer;
use bynk_syntax::parser;
use bynk_syntax::span::Span;

/// Collection-point error sink (ADR 0052). Helpers keep their plain
/// `&mut Vec<CompileError>` signatures; call sites attribute via
/// `extend_for` with the file in scope at that point.
///
/// P4.1 (#1115): relocated from `bynk-emit/src/project/diagnostics.rs`
/// alongside the `phase_*` functions above, which all take `&mut ErrorSink` —
/// the same "shared logic pulls its own types down with it" pattern already
/// applied to `UnitTable`/`ConsumedType` in the `symbols.rs` move. `Mode` and
/// `ProjectFailure` (the other two `diagnostics.rs` pipeline-driving types)
/// stayed in `bynk-emit`, unaffected — neither is a dependency of anything
/// this module needs.
pub struct ErrorSink {
    entries: Vec<AttributedError>,
    /// v0.89 (ADR 0117): non-failing warnings, classified on push by
    /// `Severity::for_error`. Kept apart so `is_empty`/`len` — the build-failure
    /// gates — stay errors-only, while every warning source (commons-fn checks,
    /// service/agent handler validation, parser) is captured uniformly.
    warnings: Vec<AttributedError>,
}

impl Default for ErrorSink {
    fn default() -> Self {
        Self::new()
    }
}

impl ErrorSink {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            warnings: Vec::new(),
        }
    }
    pub fn push_for(&mut self, file: Option<&Path>, error: CompileError) {
        let attributed = AttributedError {
            source_path: file.map(Path::to_path_buf),
            error,
        };
        match bynk_syntax::Severity::for_error(&attributed.error) {
            bynk_syntax::Severity::Warning => self.warnings.push(attributed),
            bynk_syntax::Severity::Error => self.entries.push(attributed),
        }
    }
    pub fn extend_for(
        &mut self,
        file: Option<&Path>,
        errs: impl IntoIterator<Item = CompileError>,
    ) {
        for e in errs {
            self.push_for(file, e);
        }
    }
    /// True when no **error-severity** diagnostic has been collected — the
    /// build-failure gate. Warnings do not count (ADR 0117).
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// Consume the sink, yielding the non-failing **warnings** (ADR 0117).
    pub fn into_warnings(self) -> Vec<AttributedError> {
        self.warnings
    }
    /// Consume the sink, yielding errors then warnings — the full diagnostic
    /// list the LSP and a failed build render together.
    pub fn into_all(self) -> Vec<AttributedError> {
        let mut all = self.entries;
        all.extend(self.warnings);
        all
    }
    /// The count of **error-severity** diagnostics.
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// v0.17: a resolved adapter binding — the user-authored `.binding.ts` module
/// that supplies an adapter's external provider symbols. Copied verbatim into
/// the output beside the adapter's emitted interface module so that `tsc`
/// checks the `implements` contract and compose can import the symbols.
pub struct AdapterBinding {
    /// Output path, relative to the output root (e.g. `tokens.binding.ts`).
    pub output_path: PathBuf,
    /// Verbatim TypeScript content read from the source tree.
    pub content: String,
}

/// The build target. Determines how cross-context calls and per-context
/// modules are emitted (v0.8). Bundle mode is the default — all contexts
/// emit into one TypeScript bundle and cross-context calls are direct
/// function invocations. Workers mode produces per-context Cloudflare
/// Worker bundles that communicate via Service Bindings.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum BuildTarget {
    /// Existing behaviour: one TS bundle, direct function calls between
    /// contexts.
    #[default]
    Bundle,
    /// One Worker per context. Cross-context calls become Service Binding
    /// invocations using a JSON wire format with refinement validation on
    /// the receiving side.
    Workers,
}

pub fn normalize_service_defaults(parsed: &mut [ParsedFile]) {
    for pf in parsed.iter_mut() {
        let items = match pf.unit_mut() {
            SourceUnit::Commons(c) => &mut c.items,
            SourceUnit::Context(c) => &mut c.items,
            SourceUnit::Adapter(a) => &mut a.items,
            SourceUnit::Suite(_) => continue,
        };
        for item in items.iter_mut() {
            if let CommonsItem::Service(svc) = item {
                inject_service_defaults(svc);
            }
        }
    }
}

/// Inject a single service's `by`/`given` defaults into its handlers. A handler
/// that names its own `by` (or `given`) overrides the default outright — the
/// default fills only an *absent* clause, never merges. A service with no default
/// is left untouched (byte-for-byte the pre-v0.155 behaviour).
pub fn inject_service_defaults(svc: &mut ServiceDecl) {
    let default_by = svc.default_by.clone();
    let default_given = svc.default_given.clone();
    if default_by.is_none() && default_given.is_empty() {
        return;
    }
    for handler in svc.handlers.iter_mut() {
        if handler.by_clause.is_none()
            && let Some(def) = &default_by
        {
            handler.by_clause = Some(def.clone());
        }
        if handler.given.is_empty() && !default_given.is_empty() {
            handler.given = default_given.clone();
        }
    }
}

/// Phase 1: discover the `.bynk` files under the source (and, in split mode,
/// the tests) root by walking the filesystem. Pushes any discovery error into
/// `errors` and signals a pipeline bail via `Err(())` (the caller terminates
/// with `finish`); otherwise returns the discovered `(src_files, tests_files)`.
///
/// #1077/#1081 review: this is the on-disk half only — `no_sources`/
/// `check_file_directory_conflicts` moved to [`check_discovered_files`], which
/// `run_checks` calls on the result *either* this walk *or* a caller-supplied
/// `discovered` list produces, so a `CompileOptions.sources`-driven compile
/// (the CLI's own path as of #1081) still gets both checks — they are
/// properties of "what files does this build have," not of having just
/// walked the disk to find them.
/// R3.9 (#1113): walks every `(root, prefix)` tree `Roots::trees` resolves
/// to, not a hardcoded primary/secondary pair — `trees[0]` is the mandatory
/// tree (a missing directory is a real error, via `discover_bynk_files`
/// itself); every later tree is optional, same as the old secondary tree
/// always was (a project may simply have no such subtree).
#[allow(clippy::result_unit_err)]
pub fn phase_discovery(
    trees: &[(PathBuf, PathBuf)],
    excludes: &[PathBuf],
    errors: &mut ErrorSink,
) -> Result<Vec<Vec<PathBuf>>, ()> {
    let mut out = Vec::with_capacity(trees.len());
    for (i, (root, _prefix)) in trees.iter().enumerate() {
        match discover_bynk_files(root, excludes) {
            Ok(f) => out.push(f),
            // Every tree past the first is optional — a missing directory is
            // not an error, same as the old secondary tree always was. Tried
            // via `discover_bynk_files` itself (a `fs::read_dir`) rather than
            // a `root.exists()` pre-check, which would cost a redundant
            // `stat()` per optional tree for the same answer.
            Err(e) if i > 0 && e.category == "bynk.project.no_root" => {
                out.push(Vec::new());
            }
            Err(e) => {
                errors.push_for(None, e);
                return Err(());
            }
        }
    }
    Ok(out)
}

/// The checks every tree's file list must pass regardless of where it came
/// from — a real disk walk ([`phase_discovery`]) or a caller-supplied
/// `discovered`/`CompileOptions.sources` list (#1077/#1081 review). An empty
/// project (`bynk.project.no_sources`) signals a bail via `Err(())`; a
/// file/directory name conflict is a non-fatal diagnostic.
#[allow(clippy::result_unit_err)]
pub fn check_discovered_files(
    trees: &[(PathBuf, PathBuf)],
    file_lists: &[Vec<PathBuf>],
    errors: &mut ErrorSink,
) -> Result<(), ()> {
    if file_lists.iter().all(|f| f.is_empty()) {
        errors.push_for(
            None,
            CompileError::new(
                "bynk.project.no_sources",
                Span::default(),
                format!(
                    "no `.bynk` source files found under {}",
                    trees[0].0.display()
                ),
            ),
        );
        return Err(());
    }
    for ((root, _prefix), files) in trees.iter().zip(file_lists.iter()) {
        if let Err(e) = check_file_directory_conflicts(root, files) {
            errors.extend_for(None, e);
        }
    }
    Ok(())
}

/// A memoized parse of one first-party synthetic source, keyed by the
/// call-site's own `cache` static — each of `phase_parse`'s 7 injection sites
/// below passes a distinct one. Finding #55/#65: the source text is a fixed
/// `include_str!` constant, so its parse is a pure function of that constant
/// and only needs computing once per process, not once per compile/analyse
/// round. The gating below (`consumes_bynk`, `uses_map`, etc.) is unaffected —
/// it still runs fresh for every project from that project's own parsed
/// `uses`/`consumes`; only the parse *result* being gated is cached.
/// T3.4 (R2.4): each first-party synthetic unit reserves its own 1M-wide
/// `ExprId` block, spaced far above anything a real project's own file count
/// could ever reach — see [`firstparty_parsed`]'s doc comment for why a fixed
/// reservation, not a threaded counter, is the right shape here.
pub const FIRSTPARTY_ID_BLOCK: u32 = 1_000_000;
pub const FIRSTPARTY_ID_BASE: u32 = 1_000_000_000;

pub fn firstparty_parsed(
    cache: &'static OnceLock<Result<ParsedFile, Vec<CompileError>>>,
    identity_path: &'static str,
    src: &'static str,
    kind: UnitKind,
    // T3.4 (R2.4): a fixed base, not a live project counter — `cache` is a
    // `OnceLock`, parsed once per *process*, and reused as-is across every
    // later compile in that process regardless of how many real files that
    // *particular* compile happens to have. A threaded counter can't work
    // here (this parse doesn't know, and must never depend on, which compile
    // triggers it first); a fixed, permanently-reserved range that no real
    // project could ever grow into does. Call sites space their bases
    // `FIRSTPARTY_ID_BLOCK` apart so the (currently seven) first-party units
    // can never collide with each other either, however many of them one
    // project ends up injecting together.
    id_base: u32,
) -> Result<ParsedFile, Vec<CompileError>> {
    cache
        .get_or_init(|| {
            lexer::tokenize(src)
                .map_err(|e| vec![e])
                .and_then(|toks| {
                    parser::parse_unit_with_warnings_from(&toks, src, &mut { id_base })
                        .map(|(unit, _warnings)| unit)
                })
                .map(|unit| {
                    ParsedFile::synthetic(
                        PathBuf::from(identity_path),
                        PathBuf::from(identity_path),
                        src.to_string(),
                        unit,
                        kind,
                    )
                })
        })
        .clone()
}

/// Phase 2: parse every discovered file into a `ParsedFile`, recording each
/// file's source text into `snapshots` and any parse errors into `errors`.
/// Then inject the first-party synthetic units (the `bynk`/`bynk.cloudflare`
/// adapters and the `bynk.{list,map,string}` commons) that the project
/// consumes/uses. Returns the parsed units plus whether the `bynk` and
/// `bynk.cloudflare` adapters were injected; signals a pipeline bail via
/// `Err(())` when parsing produced errors and yielded no units at all.
#[allow(clippy::too_many_arguments)]
#[allow(clippy::result_unit_err)]
pub fn phase_parse(
    // R3.9 (#1113): one `(root, prefix)` pair per `Roots::trees` entry, not a
    // hardcoded primary/secondary pair — every `include` tree is walked.
    trees: &[(PathBuf, PathBuf)],
    file_lists: &[Vec<PathBuf>],
    overlay: &HashMap<PathBuf, String>,
    errors: &mut ErrorSink,
    snapshots: &mut Vec<(PathBuf, String)>,
) -> Result<(Vec<ParsedFile>, bool, bool), ()> {
    let mut parsed: Vec<ParsedFile> = Vec::new();
    // T3.4 (R2.4): one `ExprId` counter across every file this project parse
    // touches (every tree) — see `parse_sources`'s own doc comment for why a
    // per-file counter would collide once `collect_unit_methods` merges
    // sibling files' methods together.
    let mut next_expr_id: u32 = 0;
    // T3.5 (R2.2): one `FileId` counter across every file this project parse
    // touches, mirroring `next_expr_id` above — see `parse_sources`'s own doc
    // comment.
    let mut next_file_id: u32 = 0;
    let parse_tree = |root: &Path,
                      prefix: &Path,
                      files: &[PathBuf],
                      parsed: &mut Vec<ParsedFile>,
                      errors: &mut ErrorSink,
                      snapshots: &mut Vec<(PathBuf, String)>,
                      next_expr_id: &mut u32,
                      next_file_id: &mut u32| {
        for path in files {
            // Tree-relative: what unit validation reads.
            let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();
            // Slice 0 — project-relative: what *names* the file. Equal to `rel`
            // for a single root (empty prefix).
            let id = prefix.join(&rel);
            let source = match read_source(path, overlay) {
                Ok(s) => s,
                Err(e) => {
                    errors.push_for(
                        Some(&id),
                        CompileError::new(
                            "bynk.project.read_failed",
                            Span::default(),
                            format!("could not read `{}`: {e}", path.display()),
                        ),
                    );
                    continue;
                }
            };
            snapshots.push((id.clone(), source.clone()));
            match parse_sources(root, prefix, path, source, next_expr_id, next_file_id) {
                Ok((pfs, warnings)) => {
                    parsed.extend(pfs);
                    // ADR 0117: the sink classifies these as warnings — they
                    // surface with the build but never gate it.
                    errors.extend_for(Some(&id), warnings);
                }
                Err(errs) => errors.extend_for(Some(&id), errs),
            }
        }
    };
    for ((root, prefix), files) in trees.iter().zip(file_lists.iter()) {
        parse_tree(
            root,
            prefix,
            files,
            &mut parsed,
            errors,
            snapshots,
            &mut next_expr_id,
            &mut next_file_id,
        );
    }
    if !errors.is_empty() && parsed.is_empty() {
        return Err(());
    }

    // v0.17: if any user unit consumes the first-party `bynk` surface, inject it
    // as a synthetic adapter so it flows through the normal pipeline (tables,
    // exports, emission, compose). Its binding is supplied by the toolchain for
    // the selected platform (§4.2). Injected only when consumed, so adapter-free
    // projects are unchanged.
    let consumes_bynk = parsed.iter().any(|pf| {
        pf.consumes()
            .iter()
            .any(|c| c.target.joined() == firstparty::BYNK_UNIT)
    });
    if consumes_bynk {
        static CACHE: OnceLock<Result<ParsedFile, Vec<CompileError>>> = OnceLock::new();
        match firstparty_parsed(
            &CACHE,
            "bynk.bynk",
            firstparty::BYNK_ADAPTER_SRC,
            UnitKind::Adapter,
            FIRSTPARTY_ID_BASE,
        ) {
            Ok(pf) => parsed.push(pf),
            Err(errs) => errors.extend_for(None, errs),
        }
    }
    // v0.19: likewise the first-party `bynk.cloudflare` platform adapter —
    // injected only when consumed, binding supplied by the toolchain. The
    // unit name sits inside the reserved `bynk.*` prefix (decision 0026).
    let consumes_cloudflare = parsed.iter().any(|pf| {
        pf.consumes()
            .iter()
            .any(|c| c.target.joined() == firstparty::CLOUDFLARE_UNIT)
    });
    if consumes_cloudflare {
        static CACHE: OnceLock<Result<ParsedFile, Vec<CompileError>>> = OnceLock::new();
        match firstparty_parsed(
            &CACHE,
            "bynk/cloudflare.bynk",
            firstparty::CLOUDFLARE_ADAPTER_SRC,
            UnitKind::Adapter,
            FIRSTPARTY_ID_BASE + FIRSTPARTY_ID_BLOCK,
        ) {
            Ok(pf) => parsed.push(pf),
            Err(errs) => errors.extend_for(None, errs),
        }
    }
    // v0.20b: the first-party collection commons. Unlike the adapters above
    // these are *library* units — plain Bynk commons of generic functions —
    // imported via `uses` rather than `consumes`, and injected the same way
    // so they flow through the ordinary commons pipeline (tables, uses
    // resolution, emission). `bynk.map` itself `uses bynk.list`, so using
    // the former injects both.
    let uses_unit = |parsed: &[ParsedFile], unit: &str| {
        parsed
            .iter()
            .any(|pf| pf.uses().iter().any(|u| u.target.joined() == unit))
    };
    let uses_map = uses_unit(&parsed, firstparty::MAP_UNIT);
    // `bynk.locale` itself `uses bynk.list` and `uses bynk.string`; compute it
    // up front so both injections below can OR it in the same way `uses_map`
    // is OR'd into the `bynk.list` check.
    let uses_locale = uses_unit(&parsed, firstparty::LOCALE_UNIT);
    // `bynk.locale` itself now `uses bynk.locale.types` (locale-negotiation-
    // slice-2 follow-up, #886 — split out so a context can reach `LocaleTag`
    // without also reaching `bynk.locale`'s `render`), and the `bynk` adapter
    // `uses bynk.locale.types` directly for `capability Locale`'s
    // `LocaleTag` — so this needs the same `|| uses_locale` cascade `uses_map`
    // gets from `bynk.map` into the `bynk.list` check just below.
    let uses_locale_types = uses_locale || uses_unit(&parsed, firstparty::LOCALE_TYPES_UNIT);
    if uses_map {
        static CACHE: OnceLock<Result<ParsedFile, Vec<CompileError>>> = OnceLock::new();
        match firstparty_parsed(
            &CACHE,
            "bynk/map.bynk",
            firstparty::BYNK_MAP_SRC,
            UnitKind::Commons,
            FIRSTPARTY_ID_BASE + 2 * FIRSTPARTY_ID_BLOCK,
        ) {
            Ok(pf) => parsed.push(pf),
            Err(errs) => errors.extend_for(None, errs),
        }
    }
    if uses_map || uses_locale || uses_unit(&parsed, firstparty::LIST_UNIT) {
        static CACHE: OnceLock<Result<ParsedFile, Vec<CompileError>>> = OnceLock::new();
        match firstparty_parsed(
            &CACHE,
            "bynk/list.bynk",
            firstparty::BYNK_LIST_SRC,
            UnitKind::Commons,
            FIRSTPARTY_ID_BASE + 3 * FIRSTPARTY_ID_BLOCK,
        ) {
            Ok(pf) => parsed.push(pf),
            Err(errs) => errors.extend_for(None, errs),
        }
    }
    // v0.22a: the first-party string commons — derived helpers over the
    // built-in string kernel (ADR 0046).
    if uses_locale || uses_unit(&parsed, firstparty::STRING_UNIT) {
        static CACHE: OnceLock<Result<ParsedFile, Vec<CompileError>>> = OnceLock::new();
        match firstparty_parsed(
            &CACHE,
            "bynk/string.bynk",
            firstparty::BYNK_STRING_SRC,
            UnitKind::Commons,
            FIRSTPARTY_ID_BASE + 4 * FIRSTPARTY_ID_BLOCK,
        ) {
            Ok(pf) => parsed.push(pf),
            Err(errs) => errors.extend_for(None, errs),
        }
    }
    // Locale-negotiation-slice-2 follow-up (#886): the locale value types
    // (`LocaleTag`/`MessageArg`/`Message`), split out to a dependency-free
    // leaf so `bynk.bynk`'s own `uses` (for `capability Locale`'s
    // `LocaleTag`) and a message-bundle commons's `uses bynk.locale` (for
    // `render`) no longer have to be the same clause.
    if uses_locale_types {
        static CACHE: OnceLock<Result<ParsedFile, Vec<CompileError>>> = OnceLock::new();
        match firstparty_parsed(
            &CACHE,
            "bynk/locale/types.bynk",
            firstparty::BYNK_LOCALE_TYPES_SRC,
            UnitKind::Commons,
            FIRSTPARTY_ID_BASE + 5 * FIRSTPARTY_ID_BLOCK,
        ) {
            Ok(pf) => parsed.push(pf),
            Err(errs) => errors.extend_for(None, errs),
        }
    }
    // Locale capability track, slice 1 (#844): the bundle-free `render`
    // helper and the `message`/`with*` builder API.
    if uses_locale {
        static CACHE: OnceLock<Result<ParsedFile, Vec<CompileError>>> = OnceLock::new();
        match firstparty_parsed(
            &CACHE,
            "bynk/locale.bynk",
            firstparty::BYNK_LOCALE_SRC,
            UnitKind::Commons,
            FIRSTPARTY_ID_BASE + 6 * FIRSTPARTY_ID_BLOCK,
        ) {
            Ok(pf) => parsed.push(pf),
            Err(errs) => errors.extend_for(None, errs),
        }
    }

    Ok((parsed, consumes_bynk, consumes_cloudflare))
}

/// The `include` tree that discovered `pf`, found by matching its absolute
/// path against each tree's root — not `trees[0]` unconditionally, since
/// R3.9 (#1113) lets a file live under any `include` tree, not just the
/// first. Falls back to `trees[0]` for a `pf` with no `abs_path` (unreachable
/// for a real adapter: only synthetic units, which never declare `binding`,
/// go without one) or if it somehow matches none. The longest matching root
/// wins, in case one `include` tree is nested inside another.
///
/// A `trees` root is only absolute when `Roots`'s own `project_root` is — a
/// relative project root (`bynkc build .`, the ordinary CLI shape) leaves
/// every tree root relative, while `pf.abs_path()` (`bynk-project`'s
/// `parse_sources`, via `std::path::absolute`) is always absolute. Comparing
/// them directly with `starts_with` would never match, silently collapsing
/// this back to the `trees[0]` bug it exists to fix. Each root is resolved
/// through the same `std::path::absolute` before comparing, matching
/// `abs_path`'s own normalisation exactly rather than requiring the caller
/// to have already absolutised `Roots::project_root`.
pub fn tree_root_for<'a>(trees: &'a [(PathBuf, PathBuf)], pf: &ParsedFile) -> &'a Path {
    let Some(abs) = pf.abs_path() else {
        return trees[0].0.as_path();
    };
    trees
        .iter()
        .filter_map(|(root, _)| {
            std::path::absolute(root)
                .ok()
                .map(|abs_root| (root, abs_root))
        })
        .filter(|(_, abs_root)| abs.starts_with(abs_root))
        .max_by_key(|(root, _)| root.as_os_str().len())
        .map(|(root, _)| root.as_path())
        .unwrap_or_else(|| trees[0].0.as_path())
}

/// Phase 3: group the parsed units by qualified name (production units, unit
/// tests, and integration suites tracked separately), run the per-directory
/// and path/name consistency checks, enforce the reserved `bynk` namespace and
/// the adapter `binding` rules, resolve each adapter's binding module, and fold
/// the adapters' pinned npm dependencies. Pushes diagnostics into `errors` and
/// returns the production `groups`/`kinds`, the `test`/`integration` groups, the
/// resolved `adapter_bindings`, and the collected `npm_deps`.
#[allow(clippy::type_complexity)]
#[allow(clippy::too_many_arguments)]
pub fn phase_group(
    parsed: &[ParsedFile],
    trees: &[(PathBuf, PathBuf)],
    platform: Platform,
    consumes_bynk: bool,
    consumes_cloudflare: bool,
    overlay: &HashMap<PathBuf, String>,
    // P4.1 (#1115): the whole-project function-type-boundary check
    // (`bynk-emit`'s `check_function_type_boundaries`) is one of the five
    // `validate.rs` checks the new `bynk-check`-side analysis entry point
    // deliberately omits (the documented residual gap, `analysis.rs`'s own
    // doc comment). It cannot move here itself (`validate.rs` stays in
    // `bynk-emit`), so this phase takes it as an optional hook instead of
    // calling it directly — `run_checks` passes `Some`, preserving its exact
    // diagnostic-ordering position in this phase; the new entry point passes
    // `None` and the check simply doesn't run, rather than being duplicated
    // with a different diagnostic order.
    function_type_boundary_check: Option<&dyn Fn(&[ParsedFile]) -> Vec<(PathBuf, CompileError)>>,
    errors: &mut ErrorSink,
) -> (
    BTreeMap<String, Vec<usize>>,
    BTreeMap<String, UnitKind>,
    BTreeMap<String, Vec<usize>>,
    BTreeMap<String, Vec<usize>>,
    HashMap<String, AdapterBinding>,
    std::collections::BTreeMap<String, String>,
) {
    // Tests (v0.7) are tracked separately from production units. Their
    // `target` joined-name can intentionally coincide with a commons or
    // context name; they don't enter the production groups/kinds maps.
    let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let mut kinds: BTreeMap<String, UnitKind> = BTreeMap::new();
    let mut test_groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    // v0.16: integration tests are tracked by suite name, separately again from
    // unit tests — their `name()` is the synthetic `integration <suite>`.
    let mut integration_groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, pf) in parsed.iter().enumerate() {
        let name = pf.unit().name().joined();
        if pf.kind() == UnitKind::Integration {
            integration_groups.entry(name).or_default().push(i);
        } else if pf.kind() == UnitKind::Test {
            test_groups.entry(name).or_default().push(i);
        } else {
            groups.entry(name.clone()).or_default().push(i);
            kinds.entry(name).or_insert(pf.kind());
        }
    }
    // #696: the consistency checks pair each error with the project-relative
    // path of the file its primary span belongs to, so the CLI renders them
    // with ariadne source context rather than the plain fallback.
    if let Err(e) = check_directory_name_consistency(parsed) {
        for (path, err) in e {
            errors.push_for(Some(&path), err);
        }
    }
    if let Err(e) = check_directory_kind_consistency(parsed) {
        errors.extend_for(None, e);
    }
    // A group must agree on kind across all its files (different name but
    // same kind is fine; same name but different kind is an error).
    if let Err(e) = check_group_kind_consistency(parsed, &groups) {
        for (path, err) in e {
            errors.push_for(Some(&path), err);
        }
    }
    // Each *source* unit's file path must match its declared qualified name.
    // v0.113 (DECISION S): a `suite` has no path-identity requirement — it names
    // its target and is legal in any file — so test-ness carries no path check.
    if let Err(e) = check_path_name_alignment(parsed) {
        for (path, err) in e {
            errors.push_for(Some(&path), err);
        }
    }

    // v0.20a: function types are confined to non-boundary positions. See
    // this fn's own doc comment for why this is an injected hook, not a
    // direct call.
    if let Some(check) = function_type_boundary_check {
        for (path, err) in check(parsed) {
            errors.push_for(Some(&path), err);
        }
    }

    // v0.17: the `bynk` root namespace is reserved for the toolchain. No user
    // unit of any kind may be named `bynk` or `bynk.*` (§3.4).
    for pf in parsed {
        if pf.is_synthetic() {
            continue;
        }
        let qn = pf.unit().name();
        if qn.parts.first().is_some_and(|p| p.name == "bynk") {
            errors.push_for(Some(&pf.identity_path()),
                CompileError::new(
                    "bynk.namespace.reserved",
                    qn.span,
                    format!(
                        "`{}` uses the reserved `bynk` namespace — the `bynk` root is reserved for the toolchain's conformance surface",
                        qn.joined()
                    ),
                )
                .with_note("rename the unit so its first segment is not `bynk`"),
            );
        }
    }

    // v0.17: an adapter that declares any external provider must name a
    // `binding` module to supply the implementation symbols (§3.5). First-party
    // (synthetic) adapters omit the clause — the toolchain supplies the binding.
    for pf in parsed {
        if pf.is_synthetic() {
            continue;
        }
        if let Some(a) = pf.adapter() {
            let has_external = a
                .items
                .iter()
                .any(|it| matches!(it, CommonsItem::Provider(p) if p.external));
            if has_external && a.binding.is_none() {
                errors.push_for(Some(&pf.identity_path()),
                    CompileError::new(
                        "bynk.adapter.no_binding",
                        a.span,
                        format!(
                            "adapter `{}` declares an external provider but has no `binding` clause to supply its implementation",
                            a.name.joined()
                        ),
                    )
                    .with_note(
                        "add a `binding \"<module>\"` clause naming the TypeScript module that exports the provider symbols",
                    ),
                );
            }
        }
    }

    // v0.17: resolve each adapter's binding module (relative to the adapter's
    // source file) and read it, so compose can import the external provider
    // symbols and the binding is copied into the output for the `tsc` gate.
    let mut adapter_bindings: HashMap<String, AdapterBinding> = HashMap::new();
    // v0.17: the toolchain supplies the `bynk` surface's binding, platform-keyed.
    if consumes_bynk {
        adapter_bindings.insert(
            firstparty::BYNK_UNIT.to_string(),
            AdapterBinding {
                output_path: PathBuf::from(platform.bynk_binding_filename()),
                content: platform.bynk_binding_source().to_string(),
            },
        );
    }
    // v0.19: the platform adapter's binding is single — it runs only on its
    // own platform (the lock check rejects other `--platform` selections).
    if consumes_cloudflare {
        adapter_bindings.insert(
            firstparty::CLOUDFLARE_UNIT.to_string(),
            AdapterBinding {
                output_path: PathBuf::from(firstparty::CLOUDFLARE_BINDING_FILENAME),
                content: firstparty::cloudflare_binding_source().to_string(),
            },
        );
    }
    for pf in parsed {
        let Some(a) = pf.adapter() else { continue };
        let Some(b) = &a.binding else { continue };
        let pf_source_path = pf.source_path();
        let adapter_dir = pf_source_path.parent().unwrap_or(Path::new(""));
        let out_rel = normalize_rel(&adapter_dir.join(&b.module));
        let src_abs = tree_root_for(trees, pf).join(&out_rel);
        match read_adapter_binding(&src_abs, overlay) {
            Ok(content) => {
                adapter_bindings.insert(
                    a.name.joined(),
                    AdapterBinding {
                        output_path: out_rel,
                        content,
                    },
                );
            }
            Err(e) => {
                errors.push_for(Some(&pf.identity_path()),
                    CompileError::new(
                        "bynk.adapter.no_binding",
                        b.module_span,
                        format!(
                            "adapter `{}` names binding module `{}`, which could not be read ({e})",
                            a.name.joined(),
                            b.module
                        ),
                    )
                    .with_note(
                        "the binding path is resolved relative to the adapter's source file; author the `.binding.ts` there",
                    ),
                );
            }
        }
    }

    // v0.17: collect adapter npm dependencies for `package.json`, rejecting
    // unpinned ranges ([DECISION L] stub — fold + pin-check only, no allow-list).
    let mut npm_deps: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    for pf in parsed {
        let Some(a) = pf.adapter() else { continue };
        let Some(b) = &a.binding else { continue };
        for dep in &b.requires {
            if is_unpinned_range(&dep.range) {
                errors.push_for(Some(&pf.identity_path()),
                    CompileError::new(
                        "bynk.requires.unpinned_dependency",
                        dep.span,
                        format!(
                            "dependency `{}` has an unpinned version range `{}` — pin a concrete range (e.g. `^1.2.0`)",
                            dep.package, dep.range
                        ),
                    )
                    .with_note(
                        "unpinned ranges (`*`, `latest`, …) make builds irreproducible and are rejected",
                    ),
                );
                continue;
            }
            npm_deps.insert(dep.package.clone(), dep.range.clone());
        }
    }

    (
        groups,
        kinds,
        test_groups,
        integration_groups,
        adapter_bindings,
        npm_deps,
    )
}

/// Phase 4: build each production unit's combined symbol table from its files,
/// pushing any table-construction errors into `errors`.
pub fn phase_symbol_tables(
    groups: &BTreeMap<String, Vec<usize>>,
    kinds: &BTreeMap<String, UnitKind>,
    parsed: &[ParsedFile],
    errors: &mut ErrorSink,
) -> HashMap<String, UnitTable> {
    let mut unit_tables: HashMap<String, UnitTable> = HashMap::new();
    for (name, indices) in groups {
        let kind = *kinds.get(name).expect("every group has a kind");
        // #696: build_unit_table pairs each diagnostic with its declaring file.
        let mut table_errors: Vec<(PathBuf, CompileError)> = Vec::new();
        let table = build_unit_table(name, kind, indices, parsed, &mut table_errors);
        for (path, err) in table_errors {
            errors.push_for(Some(&path), err);
        }
        unit_tables.insert(name.clone(), table);
    }
    unit_tables
}

/// Phase 5: resolve each unit's `uses` clauses, checking the target exists, is
/// a commons, and is not self-referential. Returns unit → deduplicated list of
/// used commons; diagnostics go into `errors`.
pub fn phase_resolve_uses(
    groups: &BTreeMap<String, Vec<usize>>,
    kinds: &BTreeMap<String, UnitKind>,
    parsed: &[ParsedFile],
    unit_tables: &HashMap<String, UnitTable>,
    errors: &mut ErrorSink,
) -> HashMap<String, Vec<String>> {
    let mut unit_uses: HashMap<String, Vec<String>> = HashMap::new();
    for (name, indices) in groups {
        let mut uses_targets: Vec<String> = Vec::new();
        for &i in indices {
            for u in parsed[i].uses() {
                let target = u.target.joined();
                if !unit_tables.contains_key(&target) {
                    errors.push_for(
                        Some(&parsed[i].identity_path()),
                        CompileError::new(
                            "bynk.uses.unknown_commons",
                            u.span,
                            format!("unknown commons `{target}`"),
                        )
                        .with_note(
                            "the target of a `uses` clause must be a commons in the project",
                        ),
                    );
                    continue;
                }
                let target_kind = *kinds.get(&target).unwrap();
                if target_kind != UnitKind::Commons {
                    errors.push_for(Some(&parsed[i].identity_path()),
                        CompileError::new(
                            "bynk.uses.target_is_context",
                            u.span,
                            format!(
                                "`uses {target}` targets a context — `uses` may only target a commons"
                            ),
                        )
                        .with_note(
                            "to declare a dependency on a context, use `consumes` instead",
                        ),
                    );
                    continue;
                }
                if target == *name {
                    errors.push_for(
                        Some(&parsed[i].identity_path()),
                        CompileError::new(
                            "bynk.uses.self_reference",
                            u.span,
                            format!("`{name}` cannot `uses` itself"),
                        ),
                    );
                    continue;
                }
                if !uses_targets.contains(&target) {
                    uses_targets.push(target);
                }
            }
        }
        unit_uses.insert(name.clone(), uses_targets);
    }
    unit_uses
}

/// Phase 5b: resolve each unit's `consumes` clauses (target exists, is a context
/// or adapter, not self-referential, obeys the adapter selection rules), and for
/// the braced `consumes U { Cap, … }` form validate and record the flattened
/// capabilities. Returns unit → consumed targets and unit → flattened-cap → owning
/// unit; diagnostics go into `errors` and clause-position references into `refs`.
#[allow(clippy::type_complexity)]
pub fn phase_resolve_consumes(
    groups: &BTreeMap<String, Vec<usize>>,
    kinds: &BTreeMap<String, UnitKind>,
    parsed: &[ParsedFile],
    unit_tables: &HashMap<String, UnitTable>,
    errors: &mut ErrorSink,
    refs: &mut RefSink,
) -> (
    HashMap<String, Vec<String>>,
    HashMap<String, HashMap<String, String>>,
) {
    let mut unit_consumes: HashMap<String, Vec<String>> = HashMap::new();
    // v0.17: `consumes U { Cap, … }` flattens selected caps into the consumer's
    // local namespace. unit → bare-cap → consumed unit providing it.
    let mut unit_flattened: HashMap<String, HashMap<String, String>> = HashMap::new();
    for (name, indices) in groups {
        let kind = *kinds.get(name).unwrap();
        let mut consumes_targets: Vec<String> = Vec::new();
        let mut flattened: HashMap<String, String> = HashMap::new();
        let local_caps: HashSet<String> = unit_tables
            .get(name)
            .map(|t| t.capabilities.keys().cloned().collect())
            .unwrap_or_default();
        for &i in indices {
            refs.enter_file(&parsed[i].identity_path(), name, parsed[i].is_synthetic());
            for c in parsed[i].consumes() {
                let target = c.target.joined();
                if kind != UnitKind::Context && kind != UnitKind::Adapter {
                    errors.push_for(Some(&parsed[i].identity_path()),
                        CompileError::new(
                            "bynk.consumes.in_commons",
                            c.span,
                            format!(
                                "`consumes` is only valid inside a context or adapter, not a commons `{name}`",
                            ),
                        )
                        .with_note(
                            "commons declare vocabulary; only contexts and adapters can declare behavioural dependencies",
                        ),
                    );
                    continue;
                }
                // v0.18: an adapter's `consumes` is the braced capability-selection
                // form only — an adapter has no services to RPC-call, so the
                // whole-unit and `as Alias` forms are meaningless inside one.
                if kind == UnitKind::Adapter && c.selected.is_none() {
                    errors.push_for(Some(&parsed[i].identity_path()),
                        CompileError::new(
                            "bynk.adapter.consumes_requires_selection",
                            c.span,
                            format!(
                                "an adapter's `consumes` must select capabilities — write `consumes {target} {{ Cap, … }}`",
                            ),
                        )
                        .with_note(
                            "adapters depend on capabilities, never on services; the whole-unit and aliased forms are context-only",
                        ),
                    );
                    continue;
                }
                if !unit_tables.contains_key(&target) {
                    errors.push_for(
                        Some(&parsed[i].identity_path()),
                        CompileError::new(
                            "bynk.consumes.unknown_context",
                            c.span,
                            format!("unknown context `{target}`"),
                        )
                        .with_note(
                            "the target of a `consumes` clause must be a context in the project",
                        ),
                    );
                    continue;
                }
                let target_kind = *kinds.get(&target).unwrap();
                // v0.17: `consumes` may target a context or an adapter (the host
                // boundary). It may not target a commons (use `uses` for that).
                if target_kind != UnitKind::Context && target_kind != UnitKind::Adapter {
                    errors.push_for(Some(&parsed[i].identity_path()),
                        CompileError::new(
                            "bynk.consumes.target_is_commons",
                            c.span,
                            format!(
                                "`consumes {target}` targets a commons — `consumes` may only target a context or adapter"
                            ),
                        )
                        .with_note(
                            "to mix in declarations from a commons, use `uses` instead",
                        ),
                    );
                    continue;
                }
                // v0.18: adapter dependencies are adapter-to-adapter (spec §4.5) —
                // an adapter consuming a *context* would pull service logic into
                // the host boundary.
                if kind == UnitKind::Adapter && target_kind == UnitKind::Context {
                    errors.push_for(Some(&parsed[i].identity_path()),
                        CompileError::new(
                            "bynk.adapter.consumes_context",
                            c.span,
                            format!(
                                "adapter `{name}` cannot `consumes` the context `{target}` — adapter dependencies are adapter-to-adapter"
                            ),
                        )
                        .with_note(
                            "an adapter may only depend on capabilities exported by other adapters (e.g. the `bynk` surface)",
                        ),
                    );
                    continue;
                }
                if target == *name {
                    let kind_word = if kind == UnitKind::Adapter {
                        "adapter"
                    } else {
                        "context"
                    };
                    errors.push_for(
                        Some(&parsed[i].identity_path()),
                        CompileError::new(
                            "bynk.consumes.self_reference",
                            c.span,
                            format!("{kind_word} `{name}` cannot `consumes` itself"),
                        ),
                    );
                    continue;
                }
                // v0.17: `consumes U { Cap, … }` — validate each selected name is
                // a capability `U` exports, detect clashes, and record the
                // flattening so bare `given Cap` resolves through the local path.
                if let Some(names) = &c.selected {
                    let exported = unit_tables
                        .get(&target)
                        .map(|t| &t.exported_capabilities)
                        .cloned()
                        .unwrap_or_default();
                    for cap in names {
                        if !exported.contains(&cap.name) {
                            errors.push_for(
                                Some(&parsed[i].identity_path()),
                                CompileError::new(
                                    "bynk.given.cross_context_unknown_capability",
                                    cap.span,
                                    format!(
                                        "`{target}` does not export a capability named `{}`",
                                        cap.name
                                    ),
                                ),
                            );
                            continue;
                        }
                        if local_caps.contains(&cap.name) {
                            errors.push_for(Some(&parsed[i].identity_path()), CompileError::new(
                                "bynk.consumes.capability_name_clash",
                                cap.span,
                                format!(
                                    "flattened capability `{}` clashes with a capability declared locally — use qualified `given {target}.{}` instead",
                                    cap.name, cap.name
                                ),
                            ));
                            continue;
                        }
                        if let Some(prev) = flattened.get(&cap.name) {
                            errors.push_for(Some(&parsed[i].identity_path()), CompileError::new(
                                "bynk.consumes.capability_name_clash",
                                cap.span,
                                format!(
                                    "capability `{}` is flattened from both `{prev}` and `{target}` — qualify one with `given U.{}`",
                                    cap.name, cap.name
                                ),
                            ));
                            continue;
                        }
                        // v0.25: the selection list names the capability in
                        // the consumed unit (clause-position reference).
                        refs.record_in_unit(cap.span, SymbolKind::Capability, &cap.name, &target);
                        flattened.insert(cap.name.clone(), target.clone());
                    }
                }
                if !consumes_targets.contains(&target) {
                    consumes_targets.push(target);
                }
            }
        }
        unit_consumes.insert(name.clone(), consumes_targets);
        unit_flattened.insert(name.clone(), flattened);
    }
    (unit_consumes, unit_flattened)
}

/// Phases 5b'/5b'': collect each context's `consumes` aliases (alias →
/// consumed-context name), reporting alias-vs-alias conflicts (5b'), then report
/// any alias that clashes with a locally-declared type/fn/capability/service/agent
/// (5b''). Returns the per-context alias maps; diagnostics go into `errors`.
pub fn phase_consumes_aliases(
    groups: &BTreeMap<String, Vec<usize>>,
    kinds: &BTreeMap<String, UnitKind>,
    parsed: &[ParsedFile],
    unit_tables: &HashMap<String, UnitTable>,
    errors: &mut ErrorSink,
) -> HashMap<String, HashMap<String, String>> {
    let mut unit_consumes_aliases: HashMap<String, HashMap<String, String>> = HashMap::new();
    for (name, indices) in groups {
        let kind = *kinds.get(name).unwrap();
        if kind != UnitKind::Context {
            continue;
        }
        let mut aliases: HashMap<String, String> = HashMap::new();
        let mut alias_spans: HashMap<String, Span> = HashMap::new();
        for &i in indices {
            for c in parsed[i].consumes() {
                let Some(alias) = &c.alias else { continue };
                let target = c.target.joined();
                if !unit_tables.contains_key(&target) {
                    // Already reported as unknown context above.
                    continue;
                }
                if let Some(prev_span) = alias_spans.get(&alias.name) {
                    errors.push_for(Some(&parsed[i].identity_path()),
                        CompileError::new(
                            "bynk.consumes.alias_conflict",
                            alias.span,
                            format!(
                                "alias `{}` is used by more than one `consumes` clause in context `{}`",
                                alias.name, name
                            ),
                        )
                        .with_label(*prev_span, "previously defined here")
                        .with_note(
                            "each `consumes` clause may introduce at most one alias, and aliases must be unique within a context",
                        ),
                    );
                    continue;
                }
                aliases.insert(alias.name.clone(), target);
                alias_spans.insert(alias.name.clone(), alias.span);
            }
        }
        unit_consumes_aliases.insert(name.clone(), aliases);
    }

    // -- 5b''. Detect alias-vs-local-decl conflicts. An alias must not clash
    //          with any locally declared type/fn/capability/service/agent.
    for (name, aliases) in &unit_consumes_aliases {
        let Some(local) = unit_tables.get(name) else {
            continue;
        };
        for alias in aliases.keys() {
            let alias_site = parsed_alias_span(parsed, &groups[name], alias);
            let alias_span = alias_site.map(|(_, s)| s).unwrap_or_default();
            let alias_file = alias_site.map(|(i, _)| parsed[i].identity_path());
            let conflict_kind = if local.types.contains_key(alias) {
                Some("type")
            } else if local.fns.contains_key(alias) {
                Some("function")
            } else if local.capabilities.contains_key(alias) {
                Some("capability")
            } else if local.services.contains_key(alias) {
                Some("service")
            } else if local.agents.contains_key(alias) {
                Some("agent")
            } else {
                None
            };
            if let Some(kind) = conflict_kind {
                errors.push_for(alias_file.as_deref(),
                    CompileError::new(
                        "bynk.consumes.alias_conflict",
                        alias_span,
                        format!(
                            "alias `{alias}` conflicts with a local {kind} of the same name in context `{name}`",
                        ),
                    )
                    .with_note(
                        "pick a different alias for the `consumes` clause, or rename the local declaration",
                    ),
                );
            }
        }
    }
    unit_consumes_aliases
}

/// Phase 6: for each unit, detect when two `uses`-imported commons declare the
/// same (non-shadowed) type or function name — an unrenamable conflict at the use
/// site. Diagnostics go into `errors`.
pub fn phase_uses_name_conflicts(
    unit_uses: &HashMap<String, Vec<String>>,
    unit_tables: &HashMap<String, UnitTable>,
    parsed: &[ParsedFile],
    groups: &BTreeMap<String, Vec<usize>>,
    errors: &mut ErrorSink,
) {
    for (name, targets) in unit_uses {
        let local = unit_tables.get(name).expect("unit table present");
        let mut imported: HashMap<String, String> = HashMap::new();
        for t in targets {
            let used = unit_tables.get(t).expect("used unit table present");
            for type_name in used.types.keys() {
                if local.types.contains_key(type_name) || local.fns.contains_key(type_name) {
                    continue;
                }
                if let Some(prev) = imported.get(type_name) {
                    let site = uses_span_of(parsed, &groups[name], t);
                    let span = site.map(|(_, s)| s).unwrap_or_default();
                    let file = site.map(|(i, _)| parsed[i].identity_path());
                    errors.push_for(file.as_deref(),
                        CompileError::new(
                            "bynk.uses.name_conflict",
                            span,
                            format!(
                                "`{name}` uses two commons that both declare `{type_name}`: `{prev}` and `{t}`",
                            ),
                        )
                        .with_note(
                            "name conflicts at the use site are not yet renamable; remove or restructure one of the imports",
                        ),
                    );
                } else {
                    imported.insert(type_name.clone(), t.clone());
                }
            }
            for fn_name in used.fns.keys() {
                if local.types.contains_key(fn_name) || local.fns.contains_key(fn_name) {
                    continue;
                }
                if let Some(prev) = imported.get(fn_name) {
                    let site = uses_span_of(parsed, &groups[name], t);
                    let span = site.map(|(_, s)| s).unwrap_or_default();
                    let file = site.map(|(i, _)| parsed[i].identity_path());
                    errors.push_for(file.as_deref(),
                        CompileError::new(
                            "bynk.uses.name_conflict",
                            span,
                            format!(
                                "`{name}` uses two commons that both declare `{fn_name}`: `{prev}` and `{t}`",
                            ),
                        )
                        .with_note(
                            "name conflicts at the use site are not yet renamable; remove or restructure one of the imports",
                        ),
                    );
                } else {
                    imported.insert(fn_name.clone(), t.clone());
                }
            }
        }
    }
}

/// message-bundles slice 1 (#859): messages-block legality, `@reference`
/// cardinality, within-block duplicate codes, and the `uses bynk.locale`
/// dependency. Runs here (not in `phase_group`) because it needs `unit_uses`,
/// resolved just above.
///
/// P5.0 (`design/tracks/semantics-in-the-checker.md` §6): relocated verbatim
/// from `bynk-emit/src/project/validate.rs`'s `check_messages_bundles` — one
/// of the two live editor-diagnostics regressions this slice closes (category
/// 2 of `analysis.rs`'s own seven-category accounting). Cross-locale
/// completeness (`bynk.messages.incomplete`, only for codes present in the
/// reference locale but not this one — a locale-specific-only code is not an
/// error, per the "reference is a floor, not a ceiling" convention) and
/// cross-locale placeholder-*set* agreement (`bynk.messages.placeholder_mismatch`,
/// only for codes present in both — a missing code is `incomplete`'s job, not
/// this one's). Two blocks declaring the same locale tag are rejected outright
/// (`bynk.resolve.duplicate_message_locale`, PR #875 review) — the emitter
/// has no dedup of its own, so a silent last-wins here would let a hard
/// `tsc` redeclare error (two colliding `const __messages_<tag>`
/// declarations) through instead.
pub fn phase_messages_bundles(
    parsed: &[ParsedFile],
    groups: &BTreeMap<String, Vec<usize>>,
    kinds: &BTreeMap<String, UnitKind>,
    unit_uses: &HashMap<String, Vec<String>>,
    errors: &mut ErrorSink,
) {
    for (name, indices) in groups {
        let mut first_messages: Option<(usize, Span)> = None;
        let mut reference_sites: Vec<(usize, Span)> = Vec::new();
        let mut reference_block: Option<(usize, &MessagesDecl)> = None;
        let mut by_tag: HashMap<&str, (usize, &MessagesDecl)> = HashMap::new();
        for &i in indices {
            for item in parsed[i].items() {
                let CommonsItem::Messages(m) = item else {
                    continue;
                };
                if first_messages.is_none() {
                    first_messages = Some((i, m.span));
                }
                if kinds.get(name) != Some(&UnitKind::Commons) {
                    errors.push_for(
                        Some(&parsed[i].identity_path()),
                        CompileError::new(
                            "bynk.messages.outside_commons",
                            m.span,
                            "`messages` declarations are only allowed inside a commons, not a context or adapter",
                        ),
                    );
                    continue;
                }
                // #899: the tag is a `LocaleTag` string literal, checked here
                // against `LocaleTag`'s own refinement (read from the
                // firstparty `bynk.locale.types` source, so the pattern has one
                // definition). An invalid tag would otherwise reach `Intl` at
                // runtime as `new Intl.PluralRules("xx")`, which throws — the
                // opposite of `render`'s totality contract.
                if !checker::locale_tag_accepts(&m.tag) {
                    let pattern = checker::locale_tag_pattern().unwrap_or("");
                    errors.push_for(
                        Some(&parsed[i].identity_path()),
                        CompileError::new(
                            "bynk.messages.invalid_locale_tag",
                            m.tag_span,
                            format!(
                                "\"{}\" is not a valid `LocaleTag` — it must match the pattern `{}`",
                                m.tag, pattern
                            ),
                        ),
                    );
                }
                // message-bundles slice 2 (#874, PR #875 review): two blocks
                // declaring the same locale tag are rejected, not
                // last-write-wins — the emitter (`emit_messages_bundle`) has no
                // dedup of its own and would emit two colliding table entries
                // under one object key, a hard `tsc` error. Mirrors
                // `bynk.resolve.duplicate_fn`'s own shape: only the *first*
                // occurrence seeds `by_tag`, so a third duplicate still reports
                // against the original, not the second.
                if let Some(&(_, prev)) = by_tag.get(m.tag.as_str()) {
                    errors.push_for(
                        Some(&parsed[i].identity_path()),
                        CompileError::new(
                            "bynk.resolve.duplicate_message_locale",
                            m.tag_span,
                            format!("locale \"{}\" is already declared in this bundle", m.tag),
                        )
                        .with_label(prev.tag_span, "previously declared here"),
                    );
                } else {
                    by_tag.insert(m.tag.as_str(), (i, m));
                }
                for ann in &m.annotations {
                    if ann.name.name == "reference" {
                        reference_sites.push((i, ann.span));
                        reference_block = Some((i, m));
                    }
                }
                let mut seen: HashMap<&str, Span> = HashMap::new();
                for entry in &m.entries {
                    if let Some(prev) = seen.get(entry.code.as_str()) {
                        errors.push_for(
                            Some(&parsed[i].identity_path()),
                            CompileError::new(
                                "bynk.resolve.duplicate_message_code",
                                entry.code_span,
                                format!(
                                    "message code \"{}\" is already declared in this block",
                                    entry.code
                                ),
                            )
                            .with_label(*prev, "previously declared here"),
                        );
                    } else {
                        seen.insert(entry.code.as_str(), entry.code_span);
                    }
                    // message-bundles slice 3 (#878): runs unconditionally,
                    // once per entry, regardless of `@reference` cardinality
                    // — malformed ICU syntax shouldn't wait on cardinality
                    // being resolved first.
                    check_entry_icu_syntax(entry, Some(&parsed[i].identity_path()), errors);
                }
            }
        }
        let Some((first_i, first_span)) = first_messages else {
            continue;
        };
        if kinds.get(name) != Some(&UnitKind::Commons) {
            // Already reported above (outside_commons) for every block;
            // cardinality/uses checks don't apply to a non-commons unit.
            continue;
        }
        match reference_sites.len() {
            0 => {
                errors.push_for(
                    Some(&parsed[first_i].identity_path()),
                    CompileError::new(
                        "bynk.messages.missing_reference",
                        first_span,
                        "a message bundle must have exactly one `@reference` block; none found",
                    ),
                );
            }
            1 => {
                // message-bundles slice 2 (#874): "the reference" is only
                // well-defined here — 0 or 2+ already reported their own
                // diagnostic above, and completeness/placeholder-agreement
                // against an ambiguous or absent reference would be noise.
                let (_, reference) = reference_block
                    .expect("reference_sites.len() == 1 implies reference_block is Some");
                // Sorted for deterministic diagnostic order — `by_tag`'s
                // HashMap iteration is not otherwise stable across runs.
                let mut sorted_tags: Vec<&&str> = by_tag.keys().collect();
                sorted_tags.sort();
                for &&tag in &sorted_tags {
                    let &(locale_i, locale_m) = &by_tag[tag];
                    if tag == reference.tag.as_str() {
                        continue;
                    }
                    for ref_entry in &reference.entries {
                        let Some(locale_entry) =
                            locale_m.entries.iter().find(|e| e.code == ref_entry.code)
                        else {
                            errors.push_for(
                                Some(&parsed[locale_i].identity_path()),
                                CompileError::new(
                                    "bynk.messages.incomplete",
                                    locale_m.span,
                                    format!(
                                        "locale \"{tag}\" is missing code \"{}\", declared by the reference locale \"{}\"",
                                        ref_entry.code, reference.tag
                                    ),
                                ),
                            );
                            continue;
                        };
                        let ref_names = icu::placeholder_names(&ref_entry.template);
                        let locale_names = icu::placeholder_names(&locale_entry.template);
                        if ref_names != locale_names {
                            errors.push_for(
                                Some(&parsed[locale_i].identity_path()),
                                CompileError::new(
                                    "bynk.messages.placeholder_mismatch",
                                    locale_entry.template_span,
                                    format!(
                                        "locale \"{tag}\"'s template for code \"{}\" uses placeholders {locale_names:?}, but the reference locale \"{}\"'s uses {ref_names:?}",
                                        ref_entry.code, reference.tag
                                    ),
                                ),
                            );
                        }
                        // message-bundles slice 3 (#878, Decision D): a name
                        // present in both templates must also agree on ICU
                        // format *kind* (plain/plural/select/number/date) —
                        // a UI can't sanely alternate that per locale. A
                        // missing name is `placeholder_mismatch`'s job, not
                        // this one's; a malformed template's kinds are
                        // silently absent from `template_format_kinds`
                        // (already reported once by `check_entry_icu_syntax`
                        // above, never double-reported here).
                        let ref_kinds = icu::template_format_kinds(&ref_entry.template);
                        let locale_kinds = icu::template_format_kinds(&locale_entry.template);
                        for (pname, ref_kind) in &ref_kinds {
                            let Some(locale_kind) = locale_kinds.get(pname) else {
                                continue;
                            };
                            if locale_kind != ref_kind {
                                errors.push_for(
                                    Some(&parsed[locale_i].identity_path()),
                                    CompileError::new(
                                        "bynk.messages.format_mismatch",
                                        locale_entry.template_span,
                                        format!(
                                            "locale \"{tag}\"'s placeholder \"{pname}\" in code \"{}\" is formatted as {}, but the reference locale \"{}\"'s is {}",
                                            ref_entry.code,
                                            locale_kind.as_str(),
                                            reference.tag,
                                            ref_kind.as_str(),
                                        ),
                                    ),
                                );
                            }
                        }
                    }
                }
            }
            _ => {
                let (_, first_ref_span) = reference_sites[0];
                for &(i, span) in &reference_sites[1..] {
                    errors.push_for(
                        Some(&parsed[i].identity_path()),
                        CompileError::new(
                            "bynk.messages.multiple_reference",
                            span,
                            "a message bundle must have exactly one `@reference` block; found more than one",
                        )
                        .with_label(first_ref_span, "first `@reference` here"),
                    );
                }
            }
        }
        // Locale-negotiation-slice-2 follow-up (#886): the synthetic `render`
        // this commons gets (`synthetic_render_fn`, symbols.rs) names
        // `LocaleTag`/`Message` by `TypeRef::Named` — real, resolved
        // references, not bypassed — so both `bynk.locale` (for `render`
        // itself) and `bynk.locale.types` (for the types its signature
        // names) must be `uses`d. Kept as one diagnostic, not two: a message
        // bundle always needs both together, so splitting the code would
        // just be two author-facing fixes for one underlying requirement.
        let targets = unit_uses.get(name);
        let has_locale_uses =
            targets.is_some_and(|targets| targets.iter().any(|t| t == firstparty::LOCALE_UNIT));
        let has_locale_types_uses = targets
            .is_some_and(|targets| targets.iter().any(|t| t == firstparty::LOCALE_TYPES_UNIT));
        if !has_locale_uses || !has_locale_types_uses {
            let missing = match (has_locale_uses, has_locale_types_uses) {
                (false, false) => "`bynk.locale` and `bynk.locale.types`",
                (false, true) => "`bynk.locale`",
                (true, false) => "`bynk.locale.types`",
                (true, true) => unreachable!("at least one of the two is missing here"),
            };
            errors.push_for(
                Some(&parsed[first_i].identity_path()),
                CompileError::new(
                    "bynk.messages.missing_locale_dependency",
                    first_span,
                    format!("a commons declaring `messages` must also `uses` {missing}"),
                ),
            );
        }
    }
}

/// Locale capability track, slice 2 (#882): a context whose direct `uses`
/// reaches two or more message-bundle commons has no principled single
/// answer for what `Locale.current()` should negotiate against — but this
/// is only worth diagnosing when the context actually `consumes bynk {
/// Locale }` at all; a context with 2+ bundles that never touches `Locale`
/// has nothing ambiguous to resolve.
///
/// P5.0 (`design/tracks/semantics-in-the-checker.md` §6): relocated verbatim
/// from `bynk-emit/src/project/validate.rs`'s `check_locale_bundle_ambiguity`
/// — category 3 of `analysis.rs`'s own seven-category accounting, the second
/// of this slice's two live editor-diagnostics regressions.
pub fn phase_locale_bundle_ambiguity(
    parsed: &[ParsedFile],
    groups: &BTreeMap<String, Vec<usize>>,
    kinds: &BTreeMap<String, UnitKind>,
    unit_uses: &HashMap<String, Vec<String>>,
    unit_flattened: &HashMap<String, HashMap<String, String>>,
    errors: &mut ErrorSink,
) {
    for (name, indices) in groups {
        if kinds.get(name) != Some(&UnitKind::Context) {
            continue;
        }
        let ContextMessageBundle::Many(bundles) =
            detect_context_message_bundle(name, unit_uses, groups, kinds, parsed)
        else {
            continue;
        };
        let consumes_locale = unit_flattened
            .get(name)
            .and_then(|m| m.get("Locale"))
            .is_some_and(|owner| owner == firstparty::BYNK_UNIT);
        if !consumes_locale {
            continue;
        }
        for &i in indices {
            for c in parsed[i].consumes() {
                if c.target.joined() != firstparty::BYNK_UNIT {
                    continue;
                }
                let Some(locale_ident) = c.selected.iter().flatten().find(|id| id.name == "Locale")
                else {
                    continue;
                };
                let mut err = CompileError::new(
                    "bynk.locale.multiple_message_bundles",
                    locale_ident.span,
                    format!(
                        "context `{name}` uses {} message bundles ({}) — `Locale.current()` has no single bundle to negotiate against",
                        bundles.len(),
                        bundles.join(", "),
                    ),
                );
                for &j in indices {
                    for u in parsed[j].uses() {
                        if bundles.contains(&u.target.joined()) {
                            err = err
                                .with_label(u.span, format!("`{}` used here", u.target.joined()));
                        }
                    }
                }
                errors.push_for(Some(&parsed[i].identity_path()), err);
            }
        }
    }
}

/// A message bundle entry's ICU template, syntax-checked at parse time
/// against the ICU MessageFormat grammar `icu.rs` implements. Relocated
/// alongside `phase_messages_bundles` (P5.0) — its only caller.
fn check_entry_icu_syntax(entry: &MessageEntry, file: Option<&Path>, errors: &mut ErrorSink) {
    for (inner_offset, inner) in icu::icu_dispatch_placeholders(&entry.template) {
        if let Err(e) = icu::parse_icu_placeholder(inner) {
            let decoded_start = inner_offset + e.offset;
            let decoded_span = Span::new(decoded_start, decoded_start + e.len);
            let raw_span = decoded_span.offset(entry.template_span.start + 1);
            errors.push_for(
                file,
                CompileError::new(
                    "bynk.messages.malformed_icu_syntax",
                    raw_span,
                    e.kind.message(),
                ),
            );
        }
    }
}

/// Phase 6b: validate each context/adapter's `exports opaque/transparent { … }`
/// clauses — every name must be a locally-declared type, with no duplicates
/// within a clause or conflicting visibilities across clauses. Returns unit →
/// (type → visibility); diagnostics go into `errors` and export references into
/// `refs`.
pub fn phase_validate_type_exports(
    groups: &BTreeMap<String, Vec<usize>>,
    kinds: &BTreeMap<String, UnitKind>,
    parsed: &[ParsedFile],
    unit_tables: &HashMap<String, UnitTable>,
    errors: &mut ErrorSink,
    refs: &mut RefSink,
) -> HashMap<String, HashMap<String, Visibility>> {
    let mut exports_visibility: HashMap<String, HashMap<String, Visibility>> = HashMap::new();
    for (name, indices) in groups {
        let kind = *kinds.get(name).unwrap();
        if kind != UnitKind::Context && kind != UnitKind::Adapter {
            // Commons may not have exports clauses (parsed grammar prevents it
            // at the parser level), but in case any sneak in, skip.
            continue;
        }
        let local = unit_tables.get(name).unwrap();
        let mut seen: HashMap<String, (Visibility, Span)> = HashMap::new();
        for &i in indices {
            refs.enter_file(&parsed[i].identity_path(), name, parsed[i].is_synthetic());
            for clause in parsed[i].exports() {
                // v0.15: `exports capability { ... }` clauses are validated
                // separately (§4.1); 6b handles only type exports.
                let ExportKind::Type(clause_vis) = clause.kind else {
                    continue;
                };
                let mut within: HashMap<String, Span> = HashMap::new();
                for n in &clause.names {
                    if let Some(prev) = within.get(&n.name) {
                        errors.push_for(
                            Some(&parsed[i].identity_path()),
                            CompileError::new(
                                "bynk.exports.duplicate_in_clause",
                                n.span,
                                format!(
                                    "type `{}` appears more than once in this exports clause",
                                    n.name
                                ),
                            )
                            .with_label(*prev, "previously listed here"),
                        );
                        continue;
                    }
                    within.insert(n.name.clone(), n.span);

                    if !local.types.contains_key(&n.name) {
                        errors.push_for(Some(&parsed[i].identity_path()),
                            CompileError::new(
                                "bynk.exports.undeclared_type",
                                n.span,
                                format!(
                                    "exports clause references `{}`, which is not a type declared in context `{}`",
                                    n.name, name
                                ),
                            )
                            .with_note(
                                "only types declared in the same context can appear in `exports` clauses",
                            ),
                        );
                        continue;
                    }
                    // v0.25: `exports opaque/transparent { T }` names the type.
                    refs.record(n.span, SymbolKind::Type, &n.name);

                    if let Some((prev_vis, prev_span)) = seen.get(&n.name) {
                        if *prev_vis == clause_vis {
                            errors.push_for(
                                Some(&parsed[i].identity_path()),
                                CompileError::new(
                                    "bynk.exports.duplicate_export",
                                    n.span,
                                    format!("type `{}` is exported more than once", n.name),
                                )
                                .with_label(*prev_span, "previously exported here"),
                            );
                        } else {
                            errors.push_for(Some(&parsed[i].identity_path()),
                                CompileError::new(
                                    "bynk.exports.conflicting_visibility",
                                    n.span,
                                    format!(
                                        "type `{}` is exported with conflicting visibilities — pick `opaque` or `transparent`",
                                        n.name,
                                    ),
                                )
                                .with_label(*prev_span, "previously exported here"),
                            );
                        }
                        continue;
                    }
                    seen.insert(n.name.clone(), (clause_vis, n.span));
                }
            }
        }
        let mut visibility_map: HashMap<String, Visibility> = HashMap::new();
        for (n, (v, _)) in seen {
            visibility_map.insert(n, v);
        }
        exports_visibility.insert(name.clone(), visibility_map);
    }
    exports_visibility
}

/// Phase 6b': validate each context/adapter's `exports capability { … }` clauses
/// (v0.15 §4.1) — every name must be a capability the unit declares *and*
/// provides, with no duplicate exports. Diagnostics go into `errors` and export
/// references into `refs`.
pub fn phase_validate_capability_exports(
    groups: &BTreeMap<String, Vec<usize>>,
    kinds: &BTreeMap<String, UnitKind>,
    parsed: &[ParsedFile],
    unit_tables: &HashMap<String, UnitTable>,
    errors: &mut ErrorSink,
    refs: &mut RefSink,
) {
    for (name, indices) in groups {
        if kinds.get(name) != Some(&UnitKind::Context)
            && kinds.get(name) != Some(&UnitKind::Adapter)
        {
            continue;
        }
        let local = unit_tables.get(name).unwrap();
        let mut seen: HashMap<String, Span> = HashMap::new();
        for &i in indices {
            refs.enter_file(&parsed[i].identity_path(), name, parsed[i].is_synthetic());
            for clause in parsed[i].exports() {
                if !matches!(clause.kind, ExportKind::Capability) {
                    continue;
                }
                for n in &clause.names {
                    if let Some(prev) = seen.get(&n.name) {
                        errors.push_for(
                            Some(&parsed[i].identity_path()),
                            CompileError::new(
                                "bynk.exports.duplicate_export",
                                n.span,
                                format!("capability `{}` is exported more than once", n.name),
                            )
                            .with_label(*prev, "previously exported here"),
                        );
                        continue;
                    }
                    seen.insert(n.name.clone(), n.span);
                    if local.capabilities.contains_key(&n.name) {
                        // v0.25: `exports capability { Cap }` names the
                        // capability.
                        refs.record(n.span, SymbolKind::Capability, &n.name);
                    }
                    if !local.capabilities.contains_key(&n.name) {
                        errors.push_for(Some(&parsed[i].identity_path()),
                            CompileError::new(
                                "bynk.exports.undeclared_capability",
                                n.span,
                                format!(
                                    "`exports capability` references `{}`, which is not a capability declared in context `{}`",
                                    n.name, name
                                ),
                            )
                            .with_note(
                                "only capabilities declared in the same context can appear in `exports capability` clauses",
                            ),
                        );
                        continue;
                    }
                    if !local.providers.contains_key(&n.name) {
                        errors.push_for(Some(&parsed[i].identity_path()),
                            CompileError::new(
                                "bynk.exports.capability_not_provided",
                                n.span,
                                format!(
                                    "exported capability `{}` has no provider in context `{}` — a consumer cannot instantiate it",
                                    n.name, name
                                ),
                            )
                            .with_note(
                                "add a `provides {n} = …` declaration so the capability can be wired into consumers",
                            ),
                        );
                    }
                }
            }
        }
    }
}

/// Phase 6c: validate that every (non-external) provider matches its capability
/// exactly — each capability op has a provider op, and every provider op has a
/// matching capability op with the same parameter and return types. Diagnostics
/// go into `errors`.
pub fn phase_validate_providers(
    unit_tables: &HashMap<String, UnitTable>,
    // #696: the merged `UnitTable` has flattened a unit's files away, so provider
    // diagnostics need the group's files to recover which one declares each
    // provider and attribute the diagnostic to it.
    groups: &BTreeMap<String, Vec<usize>>,
    parsed: &[ParsedFile],
    errors: &mut ErrorSink,
    tys: &Arc<Types>,
) {
    for (name, table) in unit_tables {
        // Map each provided capability to the project-relative path of the file
        // that declares its provider — every diagnostic below carries a span into
        // that file.
        let provider_files: HashMap<&str, PathBuf> = groups
            .get(name)
            .map(|indices| {
                indices
                    .iter()
                    .flat_map(|&i| {
                        parsed[i].items().iter().filter_map(move |item| match item {
                            CommonsItem::Provider(p) => {
                                Some((p.capability.name.as_str(), parsed[i].identity_path()))
                            }
                            _ => None,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        for (cap_name, provider) in &table.providers {
            let provider_file = provider_files.get(cap_name.as_str()).map(|p| p.as_path());
            // v0.17: an external provider has no Bynk body to match against the
            // capability — its implementation is the binding, checked by `tsc`.
            if provider.external {
                continue;
            }
            let Some(cap) = table.capabilities.get(cap_name) else {
                errors.push_for(provider_file,
                    CompileError::new(
                        "bynk.provider.unknown_capability",
                        provider.capability.span,
                        format!(
                            "provider targets unknown capability `{}` — declare the capability in the same context",
                            cap_name
                        ),
                    ),
                );
                continue;
            };
            // #926 (Decision E): a capability op with its own type parameter(s)
            // cannot be implemented by a Bynk-bodied provider — the body would
            // need `T` rigid through the handler-body checker for a body that
            // can only ever return `None` or echo a `T`-typed parameter.
            // External providers (checked above) are exempt: TypeScript
            // natively supports a generic interface method, so a hand-authored
            // binding class implements it directly.
            for cap_op in &cap.ops {
                if !cap_op.type_params.is_empty() {
                    errors.push_for(
                        provider_file,
                        CompileError::new(
                            "bynk.provider.generic_op_requires_external",
                            provider.span,
                            format!(
                                "provider `{}` for capability `{}` has a Bynk body, but operation `{}` declares its own type parameter(s) (`[{}]`) — a generic capability operation requires an external (bodiless) provider",
                                provider.provider_name.name,
                                cap_name,
                                cap_op.name.name,
                                cap_op
                                    .type_params
                                    .iter()
                                    .map(|p| p.name.name.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", "),
                            ),
                        )
                        .with_note(
                            "write `provides Cap = Name` with no `{ … }` block, and supply the implementation as a hand-authored class in the adapter's binding file",
                        ),
                    );
                }
            }
            // 1) Every capability op has a provider op.
            for cap_op in &cap.ops {
                if !provider.ops.iter().any(|o| o.name.name == cap_op.name.name) {
                    errors.push_for(
                        provider_file,
                        CompileError::new(
                            "bynk.provider.missing_operation",
                            provider.span,
                            format!(
                                "provider `{}` for capability `{}` is missing operation `{}`",
                                provider.provider_name.name, cap_name, cap_op.name.name
                            ),
                        ),
                    );
                }
            }
            // 2) Every provider op corresponds to a capability op with the
            //    same signature (param types and return type).
            for prov_op in &provider.ops {
                let Some(cap_op) = cap.ops.iter().find(|o| o.name.name == prov_op.name.name) else {
                    errors.push_for(provider_file, CompileError::new(
                        "bynk.provider.extra_operation",
                        prov_op.span,
                        format!(
                            "provider operation `{}.{}` does not match any operation in capability `{}`",
                            provider.provider_name.name, prov_op.name.name, cap_name
                        ),
                    ));
                    continue;
                };
                if cap_op.params.len() != prov_op.params.len() {
                    errors.push_for(provider_file, CompileError::new(
                        "bynk.provider.signature_mismatch",
                        prov_op.span,
                        format!(
                            "provider operation `{}.{}` has {} parameter(s), but capability operation expects {}",
                            provider.provider_name.name,
                            prov_op.name.name,
                            prov_op.params.len(),
                            cap_op.params.len()
                        ),
                    ));
                    continue;
                }
                // Resolved-`Ty` equality, not surface-syntax comparison: two
                // signatures that spell a type differently (an alias, or a
                // generic application written out) but resolve to the same
                // `Ty` must not be flagged as a mismatch, and — the bug this
                // replaces — a `TypeRef` shape `type_refs_match` didn't cover
                // (List/Map/Query/Stream/Connection/…) must not be silently
                // treated as *matching* just because it fell through to
                // `_ => false` on both sides of an `!`. A Bynk-bodied
                // provider op has no type params of its own (checked above),
                // so its params/return type resolve with no vars in scope.
                let cap_info = build_capability_op_info(cap_op, &table.types, tys);
                let no_vars = HashSet::new();
                let prov_params: Vec<TyId> = prov_op
                    .params
                    .iter()
                    .map(|p| {
                        checker::resolve_type_ref_in(&p.type_ref, &table.types, &no_vars, tys)
                            .unwrap_or(tys.intern(Ty::Unit))
                    })
                    .collect();
                let prov_return_ty =
                    checker::resolve_type_ref_in(&prov_op.return_type, &table.types, &no_vars, tys)
                        .unwrap_or(tys.intern(Ty::Unit));
                for (i, (cap_ty, (prov_p, prov_ty))) in cap_info
                    .params
                    .iter()
                    .zip(prov_op.params.iter().zip(prov_params.iter()))
                    .enumerate()
                {
                    if cap_ty != prov_ty {
                        errors.push_for(provider_file, CompileError::new(
                            "bynk.provider.signature_mismatch",
                            prov_p.span,
                            format!(
                                "provider operation `{}.{}` parameter {} has type `{}`, but capability declares `{}`",
                                provider.provider_name.name,
                                prov_op.name.name,
                                i + 1,
                                ts_type_ref_display(&prov_p.type_ref),
                                ts_type_ref_display(&cap_op.params[i].type_ref)
                            ),
                        ));
                    }
                }
                if cap_info.return_ty != prov_return_ty {
                    errors.push_for(provider_file, CompileError::new(
                        "bynk.provider.signature_mismatch",
                        prov_op.return_type.span(),
                        format!(
                            "provider operation `{}.{}` returns `{}`, but capability declares `{}`",
                            provider.provider_name.name,
                            prov_op.name.name,
                            ts_type_ref_display(&prov_op.return_type),
                            ts_type_ref_display(&cap_op.return_type)
                        ),
                    ));
                }
            }
        }
    }
}

/// Phase 7: build each production unit's file-declaration index (which file in
/// the unit declares which name), for cross-file lookups in the back half.
pub fn phase_file_index(
    groups: &BTreeMap<String, Vec<usize>>,
    parsed: &[ParsedFile],
) -> HashMap<String, FileDeclIndex> {
    let mut unit_file_index: HashMap<String, FileDeclIndex> = HashMap::new();
    for (name, indices) in groups {
        unit_file_index.insert(name.clone(), build_file_decl_index(indices, parsed));
    }
    unit_file_index
}

/// v0.29.4: the per-unit facets that the producer phases build as nine parallel
/// `HashMap<String, _>`s, all keyed on unit name. Assembling one record per unit
/// makes the "all these maps share one keyset" invariant structural: a single
/// lookup yields every facet as a field, so the per-column `.unwrap()`s on the
/// shared keyset disappear. Fields are total — `exports`/`aliases`/`flattened`
/// default to an empty map for a unit with no entry, reproducing the old
/// `.unwrap_or(empty)` read semantics without the dance.
pub struct UnitInfo {
    pub kind: UnitKind,
    pub table: UnitTable,
    pub uses: Vec<String>,
    pub consumes: Vec<String>,
    pub flattened: HashMap<String, String>,
    pub aliases: HashMap<String, String>,
    pub exports: HashMap<String, Visibility>,
    pub file_index: FileDeclIndex,
    pub files: Vec<usize>,
}

/// v0.29.4: fold the nine parallel per-unit maps into one `HashMap<String,
/// UnitInfo>`. Assembly is driven by the `groups` keyset (the authority), so
/// every group yields exactly one record. Facets that are genuinely optional in
/// the producer maps (`exports`/`aliases`/`flattened`, and `file_index` for a
/// unit with no declarations) default to empty — reproducing the old
/// `.unwrap_or(empty)` read semantics as a total field.
#[allow(clippy::too_many_arguments)]
pub fn assemble_unit_info(
    groups: &BTreeMap<String, Vec<usize>>,
    kinds: &BTreeMap<String, UnitKind>,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_flattened: &HashMap<String, HashMap<String, String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    exports_visibility: &HashMap<String, HashMap<String, Visibility>>,
    unit_file_index: &HashMap<String, FileDeclIndex>,
) -> BTreeMap<String, UnitInfo> {
    groups
        .iter()
        .map(|(name, indices)| {
            let info = UnitInfo {
                kind: *kinds.get(name).unwrap(),
                table: unit_tables.get(name).unwrap().clone(),
                uses: unit_uses.get(name).cloned().unwrap_or_default(),
                consumes: unit_consumes.get(name).cloned().unwrap_or_default(),
                flattened: unit_flattened.get(name).cloned().unwrap_or_default(),
                aliases: unit_consumes_aliases.get(name).cloned().unwrap_or_default(),
                exports: exports_visibility.get(name).cloned().unwrap_or_default(),
                file_index: unit_file_index
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| FileDeclIndex {
                        types: HashMap::new(),
                        fns: HashMap::new(),
                        methods: HashMap::new(),
                    }),
                files: indices.clone(),
            };
            (name.clone(), info)
        })
        .collect()
}

/// Phase 8c: collect every method authored anywhere in one unit, keyed by its
/// attached type's name — so a type's methods surface in the file that declares
/// the type even when the method lives in a sibling file. The collection loop
/// has no `continue`s, so it lifts out whole.
pub fn collect_unit_methods(
    indices: &[usize],
    parsed: &[ParsedFile],
) -> HashMap<String, Vec<FnDecl>> {
    let mut local_methods_for_type: HashMap<String, Vec<FnDecl>> = HashMap::new();
    for &j in indices {
        for item in parsed[j].items() {
            if let CommonsItem::Fn(f) = item
                && let FnName::Method { type_name, .. } = &f.name
            {
                local_methods_for_type
                    .entry(type_name.name.clone())
                    .or_default()
                    .push(f.clone());
            }
        }
    }
    local_methods_for_type
}

/// Phase 8b: merge one context's `consumes` exports into the composed symbol
/// space, recording visibility metadata in the returned `consumed_types`. The
/// per-export `continue`s (missing decl, name conflict) stay internal to the
/// loop, which lifts out whole; name conflicts are pushed into `errors` and the
/// caller's `group_error_baseline` guard reacts to them after this returns.
#[allow(clippy::too_many_arguments)]
pub fn merge_consumed_exports(
    name: &str,
    parsed: &[ParsedFile],
    unit_info: &BTreeMap<String, UnitInfo>,
    combined_types: &mut HashMap<String, Arc<TypeDecl>>,
    combined_methods: &mut HashMap<String, ResolverMethodTable>,
    imported_from: &mut HashMap<String, String>,
    imported_from_kind: &mut HashMap<String, UnitKind>,
    errors: &mut ErrorSink,
) -> HashMap<String, ConsumedType> {
    // Names visible from `consumes` (read-only types from consumed contexts).
    // For each name we track:
    // - the type decl, with the consumed context's identity
    // - the visibility (opaque/transparent)
    // - the owning context's qualified name (for external-construction errors)
    let mut consumed_types: HashMap<String, ConsumedType> = HashMap::new();

    // Now process `consumes` for contexts: add exported types into the
    // symbol table with visibility metadata so the checker can enforce
    // construction / inspection rules.
    for t in unit_info.get(name).into_iter().flat_map(|i| &i.consumes) {
        let used = &unit_info.get(t).expect("consumed unit present").table;
        let used_exports = &unit_info[t].exports;
        for (type_name, vis) in used_exports {
            let Some(decl) = used.types.get(type_name) else {
                continue;
            };
            if combined_types.contains_key(type_name) {
                // Name conflict between local/uses and consumed export.
                let consumes_site = consumes_span_of(parsed, &unit_info[name].files, t);
                let consumes_span = consumes_site.map(|(_, s)| s).unwrap_or_default();
                let consumes_file = consumes_site.map(|(i, _)| parsed[i].identity_path());
                errors.push_for(consumes_file.as_deref(),
                    CompileError::new(
                        "bynk.consumes.name_conflict",
                        consumes_span,
                        format!(
                            "context `{name}` consumes `{t}` which exports type `{type_name}`, but a type of the same name is already in scope",
                        ),
                    )
                    .with_note(
                        "rename one of the conflicting declarations or restructure the import",
                    ),
                );
                continue;
            }
            combined_types.insert(type_name.clone(), decl.clone());
            imported_from.insert(type_name.clone(), t.clone());
            imported_from_kind.insert(type_name.clone(), UnitKind::Context);
            consumed_types.insert(
                type_name.clone(),
                ConsumedType {
                    owning_context: t.clone(),
                    visibility: *vis,
                },
            );
            // Methods on transparently-exported types: they're emitted in
            // the owning context's output, but reading-side methods (like
            // user-declared instance methods) are callable from consumers.
            // For v0.4, we expose all instance methods on consumed types
            // so the checker can resolve method calls; the checker
            // separately enforces that constructors (.of/unsafe) aren't
            // callable externally.
            if let Some(mt) = used.methods.get(type_name) {
                let entry = combined_methods.entry(type_name.clone()).or_default();
                for (m, decl) in &mt.instance {
                    entry
                        .instance
                        .entry(m.clone())
                        .or_insert_with(|| decl.clone());
                }
                // We deliberately *don't* import static methods from
                // consumed contexts. Static methods can construct new
                // values, which is forbidden externally.
            }
        }
    }

    consumed_types
}

/// Phase 8a: compose one unit's symbol space — its local table plus a
/// one-level `uses` mixin (commons identity preserved). Returns the combined
/// type/fn/method tables and the `imported_from` provenance maps; the mixin
/// loop has no `continue`s, so it lifts out whole.
#[allow(clippy::type_complexity)]
pub fn compose_unit_symbols(
    name: &str,
    local_table: &UnitTable,
    unit_info: &BTreeMap<String, UnitInfo>,
) -> (
    HashMap<String, Arc<TypeDecl>>,
    HashMap<String, Arc<FnDecl>>,
    HashMap<String, ResolverMethodTable>,
    HashMap<String, String>,
    HashMap<String, UnitKind>,
) {
    // Compose: local + transitive (one level) uses. For commons, mixin
    // preserves type identity; for contexts, mixin produces per-context
    // nominal types. The resolver doesn't distinguish (the rebranding is
    // observable in emission); the symbol table union is the same.
    let mut combined_types = local_table.types.clone();
    let mut combined_fns = local_table.fns.clone();
    let mut combined_methods = local_table.methods.clone();
    let mut imported_from: HashMap<String, String> = HashMap::new();
    let mut imported_from_kind: HashMap<String, UnitKind> = HashMap::new();

    for t in unit_info.get(name).into_iter().flat_map(|i| &i.uses) {
        let used = &unit_info.get(t).expect("used unit present").table;
        for (type_name, decl) in &used.types {
            if !combined_types.contains_key(type_name) {
                combined_types.insert(type_name.clone(), decl.clone());
                imported_from.insert(type_name.clone(), t.clone());
                imported_from_kind.insert(type_name.clone(), UnitKind::Commons);
            }
        }
        for (fn_name, decl) in &used.fns {
            if !combined_fns.contains_key(fn_name) {
                combined_fns.insert(fn_name.clone(), decl.clone());
                imported_from.insert(fn_name.clone(), t.clone());
                imported_from_kind.insert(fn_name.clone(), UnitKind::Commons);
            }
        }
        for (type_name, mt) in &used.methods {
            let entry = combined_methods.entry(type_name.clone()).or_default();
            for (m, decl) in &mt.instance {
                entry
                    .instance
                    .entry(m.clone())
                    .or_insert_with(|| decl.clone());
            }
            for (m, decl) in &mt.statics {
                entry
                    .statics
                    .entry(m.clone())
                    .or_insert_with(|| decl.clone());
            }
        }
    }

    (
        combined_types,
        combined_fns,
        combined_methods,
        imported_from,
        imported_from_kind,
    )
}

/// Phase 5c: detect `consumes` cycles. #696: record each `consumes`-clause
/// site (file + span) keyed by `(consumer, target)` so a detected cycle
/// anchors on the exact clause that forms the closing edge — a real span in
/// a real file — and renders with source context. Synthetic units are left
/// out so their (snapshot-less) files never claim a diagnostic.
pub fn phase_detect_consumes_cycles(
    groups: &BTreeMap<String, Vec<usize>>,
    parsed: &[ParsedFile],
    unit_consumes: &HashMap<String, Vec<String>>,
    errors: &mut ErrorSink,
) {
    let mut consumes_sites: HashMap<(String, String), (PathBuf, Span)> = HashMap::new();
    for (name, indices) in groups {
        for &i in indices {
            if parsed[i].is_synthetic() {
                continue;
            }
            for c in parsed[i].consumes() {
                consumes_sites
                    .entry((name.clone(), c.target.joined()))
                    .or_insert_with(|| (parsed[i].identity_path(), c.span));
            }
        }
    }
    let mut cycle_errors: Vec<(Option<PathBuf>, CompileError)> = Vec::new();
    detect_consumes_cycles(unit_consumes, &consumes_sites, &mut cycle_errors);
    for (path, err) in cycle_errors {
        errors.push_for(path.as_deref(), err);
    }
}
