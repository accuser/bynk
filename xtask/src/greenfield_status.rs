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
    let test_start = trailing_test_mod_start(&lines);
    for (i, line) in lines.iter().enumerate() {
        if test_start.is_some_and(|start| i >= start) {
            continue;
        }
        if line.contains("std::fs") {
            return true;
        }
    }
    false
}

/// The line index where a trailing `#[cfg(test)] mod <ident> { ... }` block begins (the
/// `mod` line itself), or `None` if the file has no such block. Only matches a
/// brace-opening `mod` line — `#[cfg(test)] mod foo;` (an external-file declaration,
/// not an inline scope) does not count.
fn trailing_test_mod_start(lines: &[&str]) -> Option<usize> {
    for (i, line) in lines.iter().enumerate() {
        if line.trim() != "#[cfg(test)]" {
            continue;
        }
        let next = lines[i + 1..].iter().find(|l| !l.trim().is_empty())?;
        let next_trimmed = next.trim();
        if next_trimmed.starts_with("mod ") && next_trimmed.ends_with('{') {
            return lines.iter().position(|l| std::ptr::eq(*l, *next));
        }
    }
    None
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

/// ADR 0310's probe (#999 Decision E). The vendored first-party bindings under
/// `bynk-check/src/firstparty/bindings/` must reference only ADR 0310 D1's four
/// emit-ABI shapes — as they surface as import names, nine identifiers: the
/// `Result`/`Option` tag layout (`Result`, `Option`, `Ok`, `Err`, `Some`, `None`) plus
/// `JsonError`, `Uuid`, `FetchError`.
///
/// This does NOT count every non-enumerated import: a binding legitimately imports the
/// capability interfaces it implements (`Clock`, `Fetch`, `Idempotency`, `Locale`,
/// `Logger`, `Random`, `Secrets`, their message types `Request`/`Response`, and the
/// domain types `LocaleTag`, `Kv`/`KVNamespace`) — that surface is governed by
/// language-stability rules, not ADR 0310's codegen-freeze concern, and a probe that
/// flagged it would read non-zero on every binding by construction. See #999 Decision
/// E for the two-list rationale and its falsifier.
fn emit_abi_shapes(root: &Path) -> Probe {
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
            if !EMIT_ABI.contains(&ident.as_str()) && !CAPABILITY_SURFACE.contains(&ident.as_str())
            {
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
            let test_start = trailing_test_mod_start(&lines);
            for (i, line) in lines.iter().enumerate() {
                let is_blank_or_comment = line.trim().is_empty() || is_line_comment(line);
                if !is_blank_or_comment {
                    code_lines += 1;
                }
                if test_start.is_some_and(|start| i >= start) && !is_blank_or_comment {
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

/// The committed table: a plain Markdown table, probe name → gated?/reads. No rule-id
/// column yet (#999 Decision B, deferred).
pub fn render_table(report: &Report) -> String {
    let mut out = String::new();
    out.push_str("<!-- GENERATED FILE — do not edit by hand.\n");
    out.push_str("     Source: cargo xtask greenfield-status (xtask/src/greenfield_status.rs).\n");
    out.push_str("     Regenerate with: cargo xtask greenfield-status --apply -->\n\n");
    out.push_str("# Greenfield status\n\n");
    out.push_str(
        "Track slice T0.0 (#999). Nine probes are gated — a disagreement between this \
         file and a fresh run fails `greenfield_status_table_is_current` \
         (`xtask/tests/greenfield_status.rs`). Four are trend probes, reported only. No \
         rule-id (`Closes-Rule:`) column yet — that provenance is a deferred follow-on \
         slice (#999 Decision B).\n\n",
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
    out
}

/// Every gated probe whose live reading disagrees with the committed table's, as
/// `(probe name, committed, live)`. Trend probes are never compared, and never
/// computed here — this only runs the nine gated probes, so checking currency never
/// pays for `wildcard_arms`'s workspace-wide clippy pass.
pub fn gated_disagreements(root: &Path) -> Vec<(String, String, String)> {
    let committed = std::fs::read_to_string(table_path(root)).unwrap_or_default();
    let live = run_gated(root);
    let mut out = Vec::new();
    for probe in live.iter().filter(|p| p.gated) {
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

    // --- emit_abi_shapes (#999 Decision E) ----------------------------------

    /// A binding's ordinary capability-interface imports, and the emit-ABI tag-layout
    /// names, must not be flagged — the exact failure mode Decision E rebuilt the probe
    /// to avoid (the original single-allowlist definition read 29-33 here, not 1).
    #[test]
    fn emit_abi_shapes_does_not_flag_capability_or_tag_layout_imports() {
        let src = "import type { Clock, Fetch, Locale } from \"./bynk.js\";\n\
                    import { FetchError, Uuid } from \"./bynk.js\";\n\
                    import { Err, None, Ok, Some, type Option, type Result } from \"./runtime.js\";\n";
        let imports = ts_named_imports_from_runtime_modules(src);
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
        const CAPABILITY_SURFACE: &[&str] = &["Clock", "Fetch", "Locale"];
        let leaks: Vec<&String> = imports
            .iter()
            .filter(|i| {
                !EMIT_ABI.contains(&i.as_str()) && !CAPABILITY_SURFACE.contains(&i.as_str())
            })
            .collect();
        assert!(leaks.is_empty(), "unexpected leaks: {leaks:?}");
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
}
