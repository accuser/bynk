//! `cargo xtask greenfield-status` — the probe harness (track doc §8, proposal #999).
//!
//! Thirteen probes measuring the tree against `design/bynk-greenfield-compiler.md`:
//! the twelve in track doc §8, plus `emit_abi_shapes` (ADR 0310's probe, #999 Decision
//! E — this slice measures the emit-ABI enumeration guard but does not wire it; wiring
//! is packaging-track work).
//!
//! **Nine are zero/closure probes**, committed and diffed: `workspace_lints`,
//! `fs_below_driver`, `options_sources`, `hoist_sinks`, `span_keyed_maps`,
//! `emit_diagnostics`, `ide_emit_edge`, `ast_importers`, `emit_abi_shapes`. A
//! disagreement between a fresh run and the committed table fails
//! `greenfield_status_table_is_current` (`xtask/tests/greenfield_status.rs`), which
//! rides both the `test` job (`cargo test --workspace`, any Rust-touching PR) and the
//! `drift` job's existing `cargo test -p xtask` (pending/decisions-only PRs) — no new
//! CI wiring (#999 Decision D, which also explains why a `drift`-job *step* would have
//! been silently skipped on the PRs that move these probes most).
//!
//! **Four are count/ratio trend probes**, recomputed and printed but never diffed:
//! `wildcard_arms`, `keep_in_sync`, `test_density`, `fixture_kinds`. These move on
//! nearly any ordinary Rust PR (§8 calls two of them "trends, not gates"); hard-gating
//! them would make the committed table churn, and conflict, on routine work.
//!
//! `Closes-Rule:` rule-id provenance (#999 Decision B) is deferred to a follow-on
//! slice — the committed table below carries no rule-citation column yet.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

/// One probe's result. `gated` probes are diffed against the committed table by
/// [`crate::greenfield_status::gated_disagreements`]; the rest are reported only.
pub struct Probe {
    pub name: &'static str,
    pub gated: bool,
    pub reads: String,
}

pub struct Report {
    pub probes: Vec<Probe>,
}

impl Report {
    pub fn get(&self, name: &str) -> &str {
        self.probes
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.reads.as_str())
            .unwrap_or_else(|| panic!("no probe named {name:?}"))
    }
}

/// Run every probe against the tree rooted at `root` (the repo root). Used by the CLI's
/// full report; the gating test uses the nine gated probes alone
/// ([`gated_disagreements`]) so it never pays for a workspace-wide clippy pass
/// (`wildcard_arms`) just to check the probes that are actually diffed.
pub fn run(root: &Path) -> Report {
    let mut probes = run_gated(root);
    probes.extend(run_trend(root));
    Report { probes }
}

/// The nine gated (zero/closure) probes only — what [`gated_disagreements`] diffs.
fn run_gated(root: &Path) -> Vec<Probe> {
    vec![
        workspace_lints(root),
        fs_below_driver(root),
        options_sources(root),
        hoist_sinks(root),
        span_keyed_maps(root),
        emit_diagnostics(root),
        ide_emit_edge(root),
        ast_importers(root),
        emit_abi_shapes(root),
    ]
}

/// The four reported-only trend probes — never diffed, and notably including the one
/// (`wildcard_arms`) that shells out to a full `cargo clippy --workspace` pass, which
/// the gating test must not pay for on every run.
fn run_trend(root: &Path) -> Vec<Probe> {
    vec![
        wildcard_arms(root),
        keep_in_sync(root),
        test_density(root),
        fixture_kinds(root),
    ]
}

/// `design/greenfield-status.md` — the committed table this probe set regenerates.
pub fn table_path(root: &Path) -> PathBuf {
    root.join("design/greenfield-status.md")
}

// --- Filesystem helpers --------------------------------------------------

/// Every `.rs` file under `dir`, recursively, as `(path, contents)`. Unreadable files
/// (permissions, non-UTF-8) are skipped rather than failing the whole walk — this is a
/// measurement tool, not a build step.
fn rust_files(dir: &Path) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    walk(dir, &mut out);
    out
}

fn walk(dir: &Path, out: &mut Vec<(PathBuf, String)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs")
            && let Ok(contents) = std::fs::read_to_string(&path)
        {
            out.push((path, contents));
        }
    }
}

/// The inner text of every **standalone** `"bynk.<ident>"` string literal (the
/// `bynk.*` convention used for diagnostic codes and commons/namespace paths alike).
///
/// Standalone, not merely prefix-matching: the identifier run must be immediately
/// followed by the closing quote, matching the naive `rg -o '"bynk\.[a-zA-Z0-9_.]*"'`
/// this probe is deliberately more careful than (#999 Decision A). Without that
/// requirement this would also match the *start* of an unrelated, longer message that
/// merely happens to begin with "bynk." — e.g. a panic string
/// `"bynk.map itself uses bynk.list, so list must be injected too: {paths:?}"` is prose
/// beginning with a namespace-shaped word, not a `"bynk.map"` code literal, and a
/// dev-only compile-time error message split across lines with a `\`-continuation
/// (`"bynk.emit.unresolved_cross_context_signature: no signature for \` ...) is one
/// string, not a diagnostic-code literal, even though its first segment matches the
/// identifier charset. Both were found — and wrongly counted — by an earlier,
/// less careful version of this scan; the fix is requiring the closing quote.
fn bynk_dotted_literals(src: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = src.as_bytes();
    let mut i = 0;
    while let Some(rel) = src[i..].find("\"bynk.") {
        let start = i + rel + 1; // skip the opening quote
        let mut end = start;
        while end < bytes.len()
            && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_' || bytes[end] == b'.')
        {
            end += 1;
        }
        if end < bytes.len() && bytes[end] == b'"' {
            out.push(&src[start..end]);
        }
        i = end.max(start + 1);
    }
    out
}

/// True if `line`, trimmed, is a `//` or `///` or `//!` line comment. Doesn't attempt
/// block comments (`/* */`) — none of this codebase's `bynk.*`/dead-identifier
/// mentions live in one.
fn is_line_comment(line: &str) -> bool {
    line.trim_start().starts_with("//")
}

// --- Gated probe 1: workspace_lints --------------------------------------

/// R2.12. `[workspace.lints]` presence and `clippy::wildcard_enum_match_arm`'s level in
/// the root `Cargo.toml`. A boolean-shaped probe (not a count) — gated because it only
/// ever changes once, when T0.3 lands it.
fn workspace_lints(root: &Path) -> Probe {
    let cargo_toml = std::fs::read_to_string(root.join("Cargo.toml")).unwrap_or_default();
    let has_section = cargo_toml
        .lines()
        .any(|l| l.trim() == "[workspace.lints.clippy]" || l.trim() == "[workspace.lints]");
    let level = cargo_toml
        .lines()
        .find(|l| l.contains("wildcard_enum_match_arm"))
        .map(|l| l.trim().to_string());
    let reads = match (has_section, level) {
        (true, Some(l)) => format!("present — {l}"),
        (true, None) => "present, wildcard_enum_match_arm not set".to_string(),
        (false, _) => "absent".to_string(),
    };
    Probe {
        name: "workspace_lints",
        gated: true,
        reads,
    }
}

// --- Gated probe 2: fs_below_driver --------------------------------------

/// R2.3. Files under `bynk-emit/src`, `bynk-ide/src`, `bynk-fmt/src` (the crates below
/// the `bynk` driver, which owns disk I/O) that call `std::fs` in **production** code.
///
/// Excludes usage inside a trailing `#[cfg(test)] mod tests { ... }` block — the
/// convention every file in this codebase uses, always the last item in the file. A
/// line is production-scope unless it falls at or after the line following a
/// `#[cfg(test)]` attribute whose very next non-empty line opens a `mod ... {` block
/// (as opposed to a `mod name;` external-file declaration, which is not a scope at
/// all). This mirrors the comment-exclusion discipline elsewhere in this probe set:
/// tests writing fixtures to a tempdir are not "the driver's job" bypassed, and
/// counting them would report a rule open that the production code has already closed.
fn fs_below_driver(root: &Path) -> Probe {
    let crates = ["bynk-emit", "bynk-ide", "bynk-fmt"];
    let mut per_crate = Vec::new();
    let mut total = 0usize;
    for krate in crates {
        let dir = root.join(krate).join("src");
        let mut files = Vec::new();
        for (path, contents) in rust_files(&dir) {
            if has_production_std_fs(&contents) {
                files.push(
                    path.strip_prefix(root)
                        .unwrap_or(&path)
                        .display()
                        .to_string(),
                );
            }
        }
        total += files.len();
        per_crate.push(format!("{krate}={}", files.len()));
    }
    Probe {
        name: "fs_below_driver",
        gated: true,
        reads: format!("{total} files ({})", per_crate.join(", ")),
    }
}

fn has_production_std_fs(src: &str) -> bool {
    let lines: Vec<&str> = src.lines().collect();
    let ranges = test_mod_ranges(&lines);
    for (i, line) in lines.iter().enumerate() {
        if in_test_range(i, &ranges) {
            continue;
        }
        if line.contains("std::fs") {
            return true;
        }
    }
    false
}

/// Every `#[cfg(test)] mod <ident> { ... }` block in `lines`, as inclusive
/// `(start_line, end_line)` line-index ranges — every occurrence, not just a single
/// trailing block. A file in this codebase can carry several test modules scattered
/// through it with production code between them — `bynk-emit/src/emitter/lower.rs` has
/// two, 1031 lines apart, and treating "everything after the first (or last)
/// `#[cfg(test)]`" as one cutoff silently misclassifies that intervening production
/// code as test-scope (caught in review: it made `fs_below_driver`, a *gated* probe,
/// blind over that span, and inflated `test_density`'s ratio by up to 39%).
///
/// A block's end is found by real brace-depth counting via [`brace_delta`], not a
/// "first column-0 `}`" shortcut: an earlier version of this fix tried exactly that
/// shortcut (reasoning that rustfmt always dedents a closing brace back to column 0)
/// and it broke on files like `bynk-ide/src/sequence.rs`, whose test module embeds
/// multi-line `.bynk`/TypeScript fixture source as string literals — source that
/// itself contains a column-0 `}` closing a top-level construct *inside the string*,
/// which the shortcut mistook for the end of the Rust `mod` block, truncating it by
/// hundreds of lines. `brace_delta` skips characters inside Rust string/char literals
/// and comments, so embedded fixture text can't be mistaken for real Rust braces.
///
/// Only matches a brace-opening `mod` line — `#[cfg(test)] mod foo;` (an external-file
/// declaration, not an inline scope) does not open a range.
fn test_mod_ranges(lines: &[&str]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim() == "#[cfg(test)]"
            && let Some(off) = lines[i + 1..].iter().position(|l| !l.trim().is_empty())
        {
            let mod_line = i + 1 + off;
            let t = lines[mod_line].trim();
            if t.starts_with("mod ") && t.ends_with('{') {
                let mut depth = 0i32;
                let mut state = BraceScanState::Normal;
                let mut started = false;
                let mut end = lines.len() - 1;
                for (j, line) in lines[mod_line..].iter().enumerate() {
                    let (delta, new_state) = brace_delta(line, state);
                    state = new_state;
                    depth += delta;
                    if depth != 0 {
                        started = true;
                    }
                    if started && depth == 0 {
                        end = mod_line + j;
                        break;
                    }
                }
                ranges.push((mod_line, end));
                i = end + 1;
                continue;
            }
        }
        i += 1;
    }
    ranges
}

fn in_test_range(line_idx: usize, ranges: &[(usize, usize)]) -> bool {
    ranges
        .iter()
        .any(|(start, end)| line_idx >= *start && line_idx <= *end)
}

/// Scanner state carried across lines for [`brace_delta`]: whether the cursor is
/// inside a string literal, a raw string (with its `#`-count), or a block comment
/// (with nesting depth — Rust block comments nest).
#[derive(Clone, Copy, PartialEq)]
enum BraceScanState {
    Normal,
    InString,
    InRawString(u8),
    InBlockComment(u32),
}

/// The net `{`/`}` depth change in `line`, skipping characters inside Rust string/char
/// literals, raw strings, and line/block comments — a naive per-character brace count
/// breaks the moment a line contains a fixture string like `"fn f() { \"{\" }"` or a
/// doc comment mentioning a brace. Returns the depth delta and the state to carry into
/// the next line (a string or block comment can span line boundaries).
fn brace_delta(line: &str, mut state: BraceScanState) -> (i32, BraceScanState) {
    let mut delta = 0i32;
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match state {
            BraceScanState::Normal => {
                if chars[i] == '/' && chars.get(i + 1) == Some(&'/') {
                    break; // rest of the line is a line comment
                }
                if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                    state = BraceScanState::InBlockComment(1);
                    i += 2;
                    continue;
                }
                if chars[i] == '"' {
                    state = BraceScanState::InString;
                    i += 1;
                    continue;
                }
                if chars[i] == 'r' && matches!(chars.get(i + 1), Some('"') | Some('#')) {
                    let mut j = i + 1;
                    let mut hashes = 0u8;
                    while chars.get(j) == Some(&'#') {
                        hashes += 1;
                        j += 1;
                    }
                    if chars.get(j) == Some(&'"') {
                        state = BraceScanState::InRawString(hashes);
                        i = j + 1;
                        continue;
                    }
                }
                if chars[i] == '\'' {
                    // A `'\x'`/`'\\'`-style escaped char literal, or a plain `'x'` —
                    // skip past it so its contents can't be mistaken for braces.
                    // Anything else (no closing `'` within a couple of chars) is a
                    // lifetime, which owns no closing quote to skip.
                    if chars.get(i + 1) == Some(&'\\') {
                        let mut j = i + 2;
                        while j < chars.len() && chars[j] != '\'' {
                            j += 1;
                        }
                        i = (j + 1).min(chars.len());
                        continue;
                    } else if chars.get(i + 2) == Some(&'\'') {
                        i += 3;
                        continue;
                    }
                }
                match chars[i] {
                    '{' => delta += 1,
                    '}' => delta -= 1,
                    _ => {}
                }
                i += 1;
            }
            BraceScanState::InString => {
                if chars[i] == '\\' {
                    i += 2;
                    continue;
                }
                if chars[i] == '"' {
                    state = BraceScanState::Normal;
                }
                i += 1;
            }
            BraceScanState::InRawString(hashes) => {
                if chars[i] == '"' {
                    let mut j = i + 1;
                    let mut h = 0u8;
                    while chars.get(j) == Some(&'#') && h < hashes {
                        h += 1;
                        j += 1;
                    }
                    if h == hashes {
                        state = BraceScanState::Normal;
                        i = j;
                        continue;
                    }
                }
                i += 1;
            }
            BraceScanState::InBlockComment(depth) => {
                if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                    state = BraceScanState::InBlockComment(depth + 1);
                    i += 2;
                    continue;
                }
                if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    state = if depth <= 1 {
                        BraceScanState::Normal
                    } else {
                        BraceScanState::InBlockComment(depth - 1)
                    };
                    i += 2;
                    continue;
                }
                i += 1;
            }
        }
    }
    (delta, state)
}

// --- Gated probe 3: options_sources --------------------------------------

/// R2.3. `CompileOptions` (in `bynk-emit/src/project.rs`) has a `sources` field.
fn options_sources(root: &Path) -> Probe {
    let src = std::fs::read_to_string(root.join("bynk-emit/src/project.rs")).unwrap_or_default();
    let present = struct_body(&src, "CompileOptions").is_some_and(|body| body.contains("sources"));
    Probe {
        name: "options_sources",
        gated: true,
        reads: if present {
            "present".to_string()
        } else {
            "absent".to_string()
        },
    }
}

/// The `{ ... }` body text of `struct <name>` in `src`, brace-matched from the struct's
/// own opening brace to its close.
fn struct_body<'a>(src: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("struct {name}");
    let start = src.find(&needle)?;
    let open = start + src[start..].find('{')?;
    let mut depth = 0i32;
    for (offset, ch) in src[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&src[open..open + offset + 1]);
                }
            }
            _ => {}
        }
    }
    None
}

// --- Gated probe 4: hoist_sinks -------------------------------------------

/// R6.2. Live (non-comment) occurrences of the sink-passing signature
/// `stmts: &mut Vec<String>` in `bynk-emit`. Tier B (T2.1) deletes it entirely.
fn hoist_sinks(root: &Path) -> Probe {
    let dir = root.join("bynk-emit/src");
    let needle = "stmts: &mut Vec<String>";
    let mut count = 0usize;
    for (_, contents) in rust_files(&dir) {
        for line in contents.lines() {
            if !is_line_comment(line) && line.contains(needle) {
                count += 1;
            }
        }
    }
    Probe {
        name: "hoist_sinks",
        gated: true,
        reads: count.to_string(),
    }
}

// --- Gated probe 5: span_keyed_maps ---------------------------------------

/// R2.4. Whole-repo occurrences of `HashMap<Span` (comments included — the phase-3
/// migration target is every mention, not just live call sites), **excluding
/// `xtask` itself**: this probe's own doc comment and source both name the search
/// string, which would otherwise self-count every time this file is touched — the
/// same self-reference hazard flagged for the dead-identifier probes below, caught
/// here by running the probe against itself before committing the first table.
fn span_keyed_maps(root: &Path) -> Probe {
    let count = count_repo_wide(root, "HashMap<Span", &["xtask"]);
    Probe {
        name: "span_keyed_maps",
        gated: true,
        reads: count.to_string(),
    }
}

fn count_repo_wide(root: &Path, needle: &str, exclude_crates: &[&str]) -> usize {
    let mut total = 0usize;
    for entry in top_level_crate_dirs(root) {
        if exclude_crates
            .iter()
            .any(|c| entry.file_name().is_some_and(|n| n == *c))
        {
            continue;
        }
        for (_, contents) in rust_files(&entry.join("src")) {
            total += contents.matches(needle).count();
        }
    }
    total
}

/// Every workspace member crate directory (anything at the repo root with a
/// `Cargo.toml` and a `src/` dir), excluding `target` and non-crate directories.
fn top_level_crate_dirs(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && path.join("Cargo.toml").is_file() && path.join("src").is_dir() {
            out.push(path);
        }
    }
    out.sort();
    out
}

// --- Gated probe 6: emit_diagnostics --------------------------------------

/// R3.5. `bynk.*` string literals in `bynk-emit`/`bynk-check` source, cross-referenced
/// against `bynk_syntax::diagnostics::REGISTRY` — not pattern-matched. A literal not in
/// `REGISTRY` is a commons/namespace path (e.g. `bynk.locale`, the compiled first-party
/// source module name), not a diagnostic code, and must not inflate the count (#999
/// Decision A: this cross-reference is what makes the exclusion correct by
/// construction rather than a second hand-maintained list).
fn emit_diagnostics(root: &Path) -> Probe {
    let registry: BTreeSet<&str> = bynk_syntax::diagnostics::REGISTRY
        .iter()
        .map(|d| d.code)
        .collect();
    let mut parts = Vec::new();
    for (label, dir) in [
        ("bynk-emit", "bynk-emit/src"),
        ("bynk-check", "bynk-check/src"),
    ] {
        let mut naive: BTreeSet<String> = BTreeSet::new();
        for (_, contents) in rust_files(&root.join(dir)) {
            for lit in bynk_dotted_literals(&contents) {
                naive.insert(lit.to_string());
            }
        }
        let true_count = naive
            .iter()
            .filter(|l| registry.contains(l.as_str()))
            .count();
        parts.push(format!("{label}={true_count}/{}", naive.len()));
    }
    Probe {
        name: "emit_diagnostics",
        gated: true,
        reads: format!("{} (true/naive)", parts.join(", ")),
    }
}

// --- Gated probe 7: ide_emit_edge -----------------------------------------

/// R10.2. `bynk-ide` → `bynk-emit` in the manifest (`bynk-emit.workspace = true` or an
/// equivalent path/version dependency line).
fn ide_emit_edge(root: &Path) -> Probe {
    let manifest = std::fs::read_to_string(root.join("bynk-ide/Cargo.toml")).unwrap_or_default();
    let present = manifest
        .lines()
        .any(|l| l.trim_start().starts_with("bynk-emit"));
    Probe {
        name: "ide_emit_edge",
        gated: true,
        reads: if present {
            "present".to_string()
        } else {
            "absent".to_string()
        },
    }
}

// --- Gated probe 8: ast_importers -----------------------------------------

/// R6.13. Files in `bynk-emit/src` that import `bynk_syntax::ast` — phase 6 (the AST
/// import surface `bynk-emit` still depends on directly).
fn ast_importers(root: &Path) -> Probe {
    let dir = root.join("bynk-emit/src");
    let files = rust_files(&dir)
        .into_iter()
        .filter(|(_, contents)| contents.contains("bynk_syntax::ast"))
        .count();
    Probe {
        name: "ast_importers",
        gated: true,
        reads: files.to_string(),
    }
}

// --- Gated probe 9: emit_abi_shapes ---------------------------------------

/// ADR 0310 D1's four emit-ABI shapes, as they surface as import names in the vendored
/// bindings — the `Result`/`Option` tag layout plus `JsonError`, `Uuid`, `FetchError`.
const EMIT_ABI: &[&str] = &[
    "Result",
    "Option",
    "Ok",
    "Err",
    "Some",
    "None",
    "JsonError",
    "Uuid",
    "FetchError",
];

/// The capability interfaces a vendored binding legitimately imports to implement what
/// it declares — governed by language-stability rules, not ADR 0310's codegen-freeze
/// concern. See [`emit_abi_shapes`] and #999 Decision E for the two-list rationale.
const CAPABILITY_SURFACE: &[&str] = &[
    "Clock",
    "Fetch",
    "Idempotency",
    "Locale",
    "Logger",
    "Random",
    "Secrets",
    "Request",
    "Response",
    "LocaleTag",
    "Kv",
    "KVNamespace",
];

/// Is `ident` one of ADR 0310's enumerated emit-ABI shapes, or part of the capability
/// surface a binding is required to import? If neither, it's a leak `emit_abi_shapes`
/// flags — this is the single predicate both the probe and its tests use, so a test
/// asserting "no leak" can't silently pass against a list the test itself redefined.
fn is_enumerated_emit_abi_or_capability_surface(ident: &str) -> bool {
    EMIT_ABI.contains(&ident) || CAPABILITY_SURFACE.contains(&ident)
}

/// ADR 0310's probe (#999 Decision E). The vendored first-party bindings under
/// `bynk-check/src/firstparty/bindings/` must reference only [`EMIT_ABI`]'s nine names.
///
/// This does NOT count every non-enumerated import: a binding legitimately imports the
/// [`CAPABILITY_SURFACE`] interfaces it implements — that surface is governed by
/// language-stability rules, not ADR 0310's codegen-freeze concern, and a probe that
/// flagged it would read non-zero on every binding by construction. See #999 Decision
/// E for the two-list rationale and its falsifier.
fn emit_abi_shapes(root: &Path) -> Probe {
    let dir = root.join("bynk-check/src/firstparty/bindings");
    let mut leaks: Vec<String> = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Probe {
            name: "emit_abi_shapes",
            gated: true,
            reads: "bindings directory not found".to_string(),
        };
    };
    let mut files: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    files.sort();
    for path in files {
        if path.extension().is_none_or(|e| e != "ts") {
            continue;
        }
        let Ok(contents) = std::fs::read_to_string(&path) else {
            continue;
        };
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        for ident in ts_named_imports_from_runtime_modules(&contents) {
            if !is_enumerated_emit_abi_or_capability_surface(&ident) {
                leaks.push(format!("{name}:{ident}"));
            }
        }
    }
    Probe {
        name: "emit_abi_shapes",
        gated: true,
        reads: format!("{} ({})", leaks.len(), leaks.join(", ")),
    }
}

/// Named identifiers imported from the compiler-generated firstparty/runtime relative
/// modules (`./bynk.js`, `./runtime.js`, `./bynk/locale/types.js`, `./cloudflare.js`,
/// or their `../` forms) — `import type { A, B }`/`import { A, B }` braces, stripping
/// `type ` markers and `X as Y` aliases (keeping the imported name, not the local one,
/// since the allowlists are about what's referenced, not what it's called locally).
fn ts_named_imports_from_runtime_modules(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in src.lines() {
        let line = line.trim();
        if !line.starts_with("import") {
            continue;
        }
        let is_runtime_module = ["\"./bynk.js\"", "\"./runtime.js\"", "\"../runtime.js\""]
            .iter()
            .any(|m| line.ends_with(&format!("from {m};")))
            || line.contains("bynk/locale/types.js")
            || line.contains("cloudflare.js");
        if !is_runtime_module {
            continue;
        }
        let Some(open) = line.find('{') else { continue };
        let Some(close) = line.find('}') else {
            continue;
        };
        for part in line[open + 1..close].split(',') {
            let part = part.trim().trim_start_matches("type ").trim();
            if part.is_empty() {
                continue;
            }
            let imported = part.split(" as ").next().unwrap_or(part).trim();
            out.push(imported.to_string());
        }
    }
    out
}

// --- Reported probe 1: wildcard_arms --------------------------------------

/// R2.12. `clippy::wildcard_enum_match_arm` diagnostics, forced on via `-W` so the
/// count is real from day one and doesn't wait on `workspace_lints`/T0.3 (#999 Decision
/// C — delegating to clippy's own type-aware pass, rather than a hand-rolled scan for
/// "compiler-owned enum", so the probe and the enforcement mechanism can never
/// disagree). A count, not a boolean — moves on nearly every match statement anyone
/// writes, so it is reported, not gated (#999 Decision D).
fn wildcard_arms(root: &Path) -> Probe {
    let reads = match run_clippy_wildcard_scan(root) {
        Ok(n) => n.to_string(),
        Err(e) => format!("error running clippy: {e}"),
    };
    Probe {
        name: "wildcard_arms",
        gated: false,
        reads,
    }
}

/// Runs clippy with the lint forced on and parses the NDJSON output properly —
/// **not** a substring count. A single `wildcard_enum_match_arm` diagnostic's JSON
/// repeats the lint name several times (the `code` field, the human-readable message,
/// the `#[warn(...)]` note, and the `rendered` field duplicating the whole thing as
/// text), so `stdout.matches("wildcard_enum_match_arm").count()` overcounts by roughly
/// 3x — caught by cross-checking this probe's own first run against a real JSON parse
/// (296 real diagnostics, not the naive scan's 888).
///
/// Checks the process exit status: a forced `-W` (not `-D`) never fails the build on
/// account of the lint itself, so a non-zero exit means clippy genuinely could not run
/// (a compile error elsewhere, a missing toolchain component, offline with no cached
/// index) — in which case stdout carries no `compiler-message` lines and a silent
/// success would report a false, and indistinguishable, `0`. This probe is reported,
/// not gated, precisely so an honest "couldn't measure" surfaces loudly here rather
/// than being read as "closed."
fn run_clippy_wildcard_scan(root: &Path) -> std::io::Result<usize> {
    let output = Command::new("cargo")
        .args([
            "clippy",
            "--workspace",
            "--message-format=json",
            "--",
            "-W",
            "clippy::wildcard_enum_match_arm",
        ])
        .current_dir(root)
        .output()?;
    if !output.status.success() {
        return Err(std::io::Error::other(format!(
            "cargo clippy exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut count = 0usize;
    for line in stdout.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if value.get("reason").and_then(|r| r.as_str()) != Some("compiler-message") {
            continue;
        }
        let code = value.pointer("/message/code/code").and_then(|c| c.as_str());
        if code == Some("clippy::wildcard_enum_match_arm") {
            count += 1;
        }
    }
    Ok(count)
}

// --- Reported probe 2: keep_in_sync ---------------------------------------

/// P2 (trend only). Comments across the workspace containing "in sync", "mirrors",
/// "parity", or "must match" — each one names a rule the compiler cannot teach itself
/// and must be taught in review, every time.
fn keep_in_sync(root: &Path) -> Probe {
    let phrases = ["in sync", "mirrors", "parity", "must match"];
    let mut count = 0usize;
    for dir in top_level_crate_dirs(root) {
        for (_, contents) in rust_files(&dir.join("src")) {
            for line in contents.lines() {
                if is_line_comment(line) {
                    let lower = line.to_lowercase();
                    if phrases.iter().any(|p| lower.contains(p)) {
                        count += 1;
                    }
                }
            }
        }
    }
    Probe {
        name: "keep_in_sync",
        gated: false,
        reads: count.to_string(),
    }
}

// --- Reported probe 3: test_density ---------------------------------------

/// R11.1, and §3.4's phase-3 trigger. Per crate: (lines inside `#[test]` fn bodies,
/// plus lines inside `#[cfg(test)] mod` blocks outside those fns) ÷ (non-blank,
/// non-comment lines under that crate's `src/`) — #999 Decision F's definition,
/// written down precisely because an undefined "ratio" is exactly the ambiguity that
/// produced the track doc §9's four-row ambiguity.
fn test_density(root: &Path) -> Probe {
    let mut parts = Vec::new();
    for dir in top_level_crate_dirs(root) {
        let name = dir.file_name().unwrap().to_string_lossy().to_string();
        let src_dir = dir.join("src");
        let mut test_lines = 0usize;
        let mut code_lines = 0usize;
        for (_, contents) in rust_files(&src_dir) {
            let lines: Vec<&str> = contents.lines().collect();
            let ranges = test_mod_ranges(&lines);
            for (i, line) in lines.iter().enumerate() {
                let is_blank_or_comment = line.trim().is_empty() || is_line_comment(line);
                if !is_blank_or_comment {
                    code_lines += 1;
                }
                if in_test_range(i, &ranges) && !is_blank_or_comment {
                    test_lines += 1;
                }
            }
        }
        if code_lines > 0 {
            let ratio = 100.0 * test_lines as f64 / code_lines as f64;
            parts.push(format!("{name}={ratio:.1}%"));
        }
    }
    Probe {
        name: "test_density",
        gated: false,
        reads: parts.join(", "),
    }
}

// --- Reported probe 4: fixture_kinds --------------------------------------

/// R11.2. Fixture directories under `bynkc/tests` using each assertion granularity —
/// `expected_contains.txt` / `expected_absent.txt` / `expected_diagnostics.txt` — set
/// against the older, coarser `expected_error.txt` (category-string) convention.
fn fixture_kinds(root: &Path) -> Probe {
    let tests_dir = root.join("bynkc/tests");
    let contains = count_files_named(&tests_dir, "expected_contains.txt");
    let absent = count_files_named(&tests_dir, "expected_absent.txt");
    let diagnostics = count_files_named(&tests_dir, "expected_diagnostics.txt");
    let error = count_files_named(&tests_dir, "expected_error.txt");
    Probe {
        name: "fixture_kinds",
        gated: false,
        reads: format!(
            "contains={contains}, absent={absent}, diagnostics={diagnostics}, error={error}"
        ),
    }
}

fn count_files_named(dir: &Path, filename: &str) -> usize {
    let mut count = 0usize;
    count_files_named_walk(dir, filename, &mut count);
    count
}

fn count_files_named_walk(dir: &Path, filename: &str, count: &mut usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            count_files_named_walk(&path, filename, count);
        } else if path.file_name().is_some_and(|n| n == filename) {
            *count += 1;
        }
    }
}

// --- Rendering + diffing ---------------------------------------------------

/// The committed table: a plain Markdown table, probe name → gated?/reads, plus a
/// summary of the rule ledger `stamp::apply` writes (#1001).
pub fn render_table(report: &Report, root: &Path) -> String {
    let mut out = String::new();
    out.push_str("<!-- GENERATED FILE — do not edit by hand.\n");
    out.push_str("     Source: cargo xtask greenfield-status (xtask/src/greenfield_status.rs).\n");
    out.push_str("     Regenerate with: cargo xtask greenfield-status --apply -->\n\n");
    out.push_str("# Greenfield status\n\n");
    out.push_str(
        "Track slice T0.0 (#999). Nine probes are gated — a disagreement between this \
         file and a fresh run fails `greenfield_status_table_is_current` \
         (`xtask/tests/greenfield_status.rs`). Four are trend probes, reported only.\n\n",
    );
    out.push_str("| Probe | Gated | Reads |\n|---|---|---|\n");
    for probe in &report.probes {
        let _ = writeln!(
            out,
            "| `{}` | {} | {} |",
            probe.name,
            if probe.gated { "yes" } else { "no (trend)" },
            probe.reads
        );
    }

    out.push_str("\n## Rules closed\n\n");
    // Summarised, not duplicated: the ledger is `stamp::apply`'s own file
    // (#1001) and this regenerates wholesale on every `--apply`, so embedding
    // its rows here would be a second copy that can only drift from the
    // original — count and link instead.
    match std::fs::read_to_string(root.join("design/greenfield-status-rules.md")) {
        Ok(ledger) => {
            // A rule-id row starts `| R<digit>` (e.g. `| R2.3 |`) — not just
            // `| R`, which the table's own header (`| Rule | Version | ...`)
            // also starts with and would otherwise be miscounted as a row.
            let rows = ledger
                .lines()
                .filter(|l| {
                    l.as_bytes().get(3).is_some_and(u8::is_ascii_digit) && l.starts_with("| R")
                })
                .count();
            let _ = writeln!(
                out,
                "{rows} rule id(s) closed so far — see \
                 [`design/greenfield-status-rules.md`](greenfield-status-rules.md)."
            );
        }
        Err(_) => out.push_str(
            "No increment has cited `closes_rule` yet — `design/greenfield-status-rules.md` \
             does not exist.\n",
        ),
    }
    out
}

/// Every gated probe whose live reading disagrees with the committed table's, as
/// `(probe name, committed, live)`. Trend probes are never compared, and never
/// computed here — this only runs the nine gated probes, so checking currency never
/// pays for `wildcard_arms`'s workspace-wide clippy pass. For a caller that has already
/// run the full report (e.g. to print it), use [`gated_disagreements_in`] instead so the
/// nine gated probes aren't computed a second time.
pub fn gated_disagreements(root: &Path) -> Vec<(String, String, String)> {
    gated_disagreements_in(&run_gated(root), root)
}

/// Like [`gated_disagreements`], but diffs `probes` (typically a [`Report`]'s
/// `.probes`, already computed) instead of re-running the gated probes.
pub fn gated_disagreements_in(probes: &[Probe], root: &Path) -> Vec<(String, String, String)> {
    let committed = std::fs::read_to_string(table_path(root)).unwrap_or_default();
    let mut out = Vec::new();
    for probe in probes.iter().filter(|p| p.gated) {
        let row_prefix = format!("| `{}` | yes | ", probe.name);
        let committed_reads = committed
            .lines()
            .find(|l| l.starts_with(&row_prefix))
            .and_then(|l| l.strip_prefix(&row_prefix))
            .and_then(|l| l.strip_suffix(" |"))
            .unwrap_or("<row missing>");
        if committed_reads != probe.reads {
            out.push((
                probe.name.to_string(),
                committed_reads.to_string(),
                probe.reads.clone(),
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- emit_diagnostics (#999 Decision A) ---------------------------------

    /// A standalone `"bynk.foo"` literal is found — the ordinary case.
    #[test]
    fn bynk_dotted_literals_finds_standalone_literal() {
        let src = r#"code("bynk.check.something", "a message")"#;
        assert_eq!(bynk_dotted_literals(src), vec!["bynk.check.something"]);
    }

    /// The bug this slice found in its own first draft: a longer message that merely
    /// *starts* with "bynk." must not be truncated into a fake code literal. Regression
    /// test for `bynk.map itself uses bynk.list, so list must be injected too: {paths:?}`
    /// (`bynk-emit/src/project.rs`), which an earlier, less careful version of this scan
    /// wrongly counted as the literal `"bynk.map"`.
    #[test]
    fn bynk_dotted_literals_ignores_prefix_of_a_longer_message() {
        let src = r#"assert!(cond, "bynk.map itself uses bynk.list, so list must be injected too: {paths:?}");"#;
        assert!(bynk_dotted_literals(src).is_empty());
    }

    /// Regression test for the other half of the same bug: a `\`-continued string
    /// literal (`"bynk.emit.unresolved_cross_context_signature: no signature for \`,
    /// continued on the next source line) is one string, not a diagnostic-code literal,
    /// even though its first segment matches the identifier charset — because the
    /// character after the run is `:`, never a closing quote, on either line.
    #[test]
    fn bynk_dotted_literals_ignores_a_line_continued_message() {
        let src =
            "\"bynk.emit.unresolved_cross_context_signature: no signature for \\\n     the rest\"";
        assert!(bynk_dotted_literals(src).is_empty());
    }

    /// The whole point of Decision A: cross-referencing the real registry, not a
    /// hand-maintained exclusion list, correctly separates a real diagnostic code from
    /// a commons/namespace path that merely looks like one.
    #[test]
    fn emit_diagnostics_cross_references_the_real_registry() {
        let registry: BTreeSet<&str> = bynk_syntax::diagnostics::REGISTRY
            .iter()
            .map(|d| d.code)
            .collect();
        // A code this registry is known to carry (bynk-syntax/src/diagnostics.rs).
        assert!(registry.contains("bynk.parse.expected_expression"));
        // A commons/namespace path, not a diagnostic code — #999's own verified survey.
        assert!(!registry.contains("bynk.locale"));
    }

    // --- fs_below_driver / test_density (trailing `#[cfg(test)] mod tests {}`) ---

    #[test]
    fn production_std_fs_usage_is_detected() {
        let src = "fn load(p: &Path) -> String {\n    std::fs::read_to_string(p).unwrap()\n}\n";
        assert!(has_production_std_fs(src));
    }

    #[test]
    fn std_fs_inside_a_trailing_test_mod_is_not_production() {
        let src = "fn load(p: &Path) -> String {\n    String::new()\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {\n        std::fs::write(\"x\", \"y\").unwrap();\n    }\n}\n";
        assert!(!has_production_std_fs(src));
    }

    /// Regression test for the other real bug this slice found: `bynk-emit/src/lib.rs`
    /// has `#[cfg(test)] pub(crate) mod testkit;` — an external-file module
    /// *declaration* (ends in `;`), not an inline block. It must not be mistaken for a
    /// scope-opening `mod tests { ... }`, or the (genuinely production) code after it in
    /// the same file would be wrongly excluded.
    #[test]
    fn cfg_test_external_mod_declaration_does_not_open_a_test_region() {
        let src = "#[cfg(test)]\npub(crate) mod testkit;\n\nfn load(p: &Path) -> String {\n    std::fs::read_to_string(p).unwrap()\n}\n";
        assert!(has_production_std_fs(src));
    }

    /// Regression test for the bug caught in review: a file with **two** scattered
    /// `#[cfg(test)] mod ... { ... }` blocks, with real production code between them —
    /// exactly `bynk-emit/src/emitter/lower.rs`'s shape (two test modules, 1031
    /// production lines apart). A single "everything from the first/last `#[cfg(test)]`
    /// onward" cutoff would misclassify `lower_lambda` here as test-scope; the fix must
    /// close each block at its own boundary and resume production scanning after it.
    #[test]
    fn production_code_between_two_scattered_test_mods_is_detected() {
        let src = "\
#[cfg(test)]
mod decode_map_key_tests {
    #[test]
    fn t() {
        assert_eq!(1, 1);
    }
}

fn lower_lambda(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap()
}

#[cfg(test)]
mod idempotency_scoping_tests {
    #[test]
    fn t2() {
        assert_eq!(2, 2);
    }
}
";
        assert!(has_production_std_fs(src));
    }

    /// The same fixture's `test_mod_ranges` shape, checked directly: two disjoint
    /// ranges, not one span from the first block to the last.
    #[test]
    fn test_mod_ranges_finds_each_block_separately() {
        let src = "\
#[cfg(test)]
mod a {
    fn x() {}
}

fn production() {}

#[cfg(test)]
mod b {
    fn y() {}
}
";
        let lines: Vec<&str> = src.lines().collect();
        let ranges = test_mod_ranges(&lines);
        assert_eq!(
            ranges.len(),
            2,
            "expected two disjoint test-mod ranges: {ranges:?}"
        );
        // Line 5 (0-indexed) is `fn production() {}`, between the two blocks.
        assert!(
            !in_test_range(5, &ranges),
            "production() must not read as test-scope"
        );
    }

    /// Regression test for the bug in the *fix* for the above: a column-0-`}`
    /// shortcut (tried and reverted during review) truncates a test module the moment
    /// its body embeds a multi-line fixture string containing a `}` flush against the
    /// left margin — exactly `bynk-ide/src/sequence.rs`'s shape, whose test mod embeds
    /// `.bynk` source fixtures. The real brace-depth scanner must see through the
    /// string and find the module's *actual* closing brace, hundreds of lines later.
    /// Uses a raw string for the outer fixture so the embedded `"..."` doesn't need
    /// escaping, and locates the real end by content rather than a hand-counted index
    /// — a hand-counted line number is exactly the kind of easy-to-miscount detail
    /// this codebase's own convention (verify, don't assume) warns against.
    #[test]
    fn test_mod_ranges_is_not_fooled_by_a_column_zero_brace_inside_a_string() {
        let src = r#"#[cfg(test)]
mod tests {
    const FIXTURE: &str = "
commons app.demo {
}
";

    fn real_end_of_module() {}
}
"#;
        let lines: Vec<&str> = src.lines().collect();
        let ranges = test_mod_ranges(&lines);
        assert_eq!(ranges.len(), 1, "expected exactly one range: {ranges:?}");
        let (_, end) = ranges[0];
        // `str::lines()` drops the trailing newline, so the module's real closing
        // brace — the fixture's last line — is at `lines.len() - 1`. The string's
        // embedded `}` (an earlier line) must not be mistaken for it.
        assert_eq!(
            end,
            lines.len() - 1,
            "closed too early — mistook the string's `}}` for the module's: {ranges:?}"
        );
    }

    // --- emit_abi_shapes (#999 Decision E) ----------------------------------

    /// A binding's ordinary capability-interface imports, and the emit-ABI tag-layout
    /// names, must not be flagged — the exact failure mode Decision E rebuilt the probe
    /// to avoid (the original single-allowlist definition read 29-33 here, not 1).
    ///
    /// Exercises the real production allowlists via [`is_enumerated_emit_abi_or_capability_surface`]
    /// — not a local re-declaration. A test with its own copy of `EMIT_ABI` would still
    /// pass if the real one lost an entry (e.g. deleting `Uuid` from the production
    /// list), proving nothing about the probe it claims to cover.
    #[test]
    fn emit_abi_shapes_does_not_flag_capability_or_tag_layout_imports() {
        let src = "import type { Clock, Fetch, Locale } from \"./bynk.js\";\n\
                    import { FetchError, Uuid } from \"./bynk.js\";\n\
                    import { Err, None, Ok, Some, type Option, type Result } from \"./runtime.js\";\n";
        let imports = ts_named_imports_from_runtime_modules(src);
        let leaks: Vec<&String> = imports
            .iter()
            .filter(|i| !is_enumerated_emit_abi_or_capability_surface(i))
            .collect();
        assert!(leaks.is_empty(), "unexpected leaks: {leaks:?}");
    }

    /// The falsifier from #999 Decision E, checked directly: deleting an entry from the
    /// real production allowlist must be detectable by *some* test — this one flags
    /// `Uuid` as a leak the moment it's removed from [`EMIT_ABI`], which the test above
    /// (using the real const) would also start failing on.
    #[test]
    fn is_enumerated_checks_the_real_production_allowlist() {
        assert!(is_enumerated_emit_abi_or_capability_surface("Uuid"));
        assert!(is_enumerated_emit_abi_or_capability_surface("LocaleTag"));
        assert!(!is_enumerated_emit_abi_or_capability_surface(
            "negotiateLocale"
        ));
    }

    /// The real, current-tree finding this probe exists to surface: `negotiateLocale`,
    /// a plain value helper from `./runtime.js` alongside the tag-layout constructors,
    /// is neither an enumerated emit-ABI shape nor a capability-interface import.
    #[test]
    fn emit_abi_shapes_flags_a_non_enumerated_runtime_helper() {
        let src = "import { Err, None, Ok, Some, negotiateLocale, type Option, type Result } from \"./runtime.js\";\n";
        let imports = ts_named_imports_from_runtime_modules(src);
        assert!(imports.contains(&"negotiateLocale".to_string()));
    }

    /// `FetchError` is `import type` in one binding and a plain value import in
    /// another (`FetchError.Timeout`) — Decision E's rejected type-vs-value
    /// discriminator. Confirms the extractor treats both forms as the same identifier,
    /// so the allowlist check doesn't depend on which form a given file happens to use.
    #[test]
    fn ts_import_extraction_ignores_type_only_vs_value_distinction() {
        let type_only = "import type { FetchError } from \"./bynk.js\";\n";
        let value = "import { FetchError, Uuid } from \"./bynk.js\";\n";
        assert_eq!(
            ts_named_imports_from_runtime_modules(type_only),
            vec!["FetchError".to_string()]
        );
        assert!(ts_named_imports_from_runtime_modules(value).contains(&"FetchError".to_string()));
    }

    // --- options_sources -----------------------------------------------------

    #[test]
    fn struct_body_finds_a_field_by_name() {
        let src = "struct Foo {\n    pub sources: Option<HashMap<PathBuf, String>>,\n    pub other: bool,\n}\n";
        let body = struct_body(src, "Foo").expect("struct body found");
        assert!(body.contains("sources"));
    }

    #[test]
    fn struct_body_does_not_match_an_unrelated_struct() {
        let src =
            "struct Bar {\n    pub sources: bool,\n}\n\nstruct Foo {\n    pub other: bool,\n}\n";
        let body = struct_body(src, "Foo").expect("struct body found");
        assert!(!body.contains("sources"));
    }

    // --- render_table's "Rules closed" section (#1001) ------------------------

    fn empty_report() -> Report {
        Report { probes: Vec::new() }
    }

    #[test]
    fn render_table_reports_no_ledger_when_none_exists() {
        let root = std::env::temp_dir().join("xtask-render-table-no-ledger");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let out = render_table(&empty_report(), &root);
        assert!(
            out.contains("No increment has cited `closes_rule` yet"),
            "{out}"
        );
    }

    #[test]
    fn render_table_counts_ledger_rows_without_duplicating_them() {
        let root = std::env::temp_dir().join("xtask-render-table-with-ledger");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("design")).unwrap();
        std::fs::write(
            root.join("design/greenfield-status-rules.md"),
            "# Rules closed\n\n| Rule | Version | PR | Changelog |\n|---|---|---|---|\n\
             | R2.3 | v0.246.3 | #1001 | Add the ledger |\n\
             | R2.4 | v0.246.4 | #1005 | Close another |\n",
        )
        .unwrap();
        let out = render_table(&empty_report(), &root);
        assert!(out.contains("2 rule id(s) closed"), "{out}");
        // Summarised, not duplicated — the individual rows must not appear here.
        assert!(!out.contains("Add the ledger"), "{out}");
        assert!(out.contains("greenfield-status-rules.md"), "{out}");
    }
}
