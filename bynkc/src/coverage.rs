//! `bynkc test --coverage` — remap V8 line coverage onto `.bynk` source.
//!
//! The test runner already owns the two artefacts a coverage tool needs and a
//! user cannot reconstruct: it launches the `node` process that executes the
//! suite, and it holds the source maps from `.bynk` → emitted `.ts`. This module
//! is the "one genuinely new piece" the coverage proposal (issue #854, ADR
//! recorded at merge) calls out: it reads the raw V8 coverage the runtime writes
//! to `NODE_V8_COVERAGE`, and attributes each executed / unexecuted line back to
//! `.bynk` source through **two** line-level source-map hops:
//!
//! 1. `out-js/**/*.js.map` — tsc's map, `.js` line → emitted `.ts` line. `tsc`
//!    does **not** chain input maps, so this hop only reaches the `.ts`.
//! 2. `out/**/*.ts.map` — the emitter's map (ADR 0103), `.ts` line → `.bynk`
//!    line. Statement-anchored and line-level (generated column always 0).
//!
//! Composed, a covered `.js` line lands on a `.bynk` line. Emitted glue with no
//! `.bynk` origin (codec wrappers, capability injection, the module header) is
//! **unmapped** in hop 2, so it contributes nothing — it is counted as
//! out-of-scope, never as uncovered user code (the proposal's map-fidelity
//! mitigation, for free).
//!
//! **Decisions realised here** (recorded in the ADR): line/statement coverage
//! only, no branch coverage (DECISION B) — a `.bynk` line is *covered* if any
//! generated line mapping to it executed; and the measured set excludes the
//! `tests/` tree and the workers scaffold (DECISION D), filtered on the emitted
//! `out-js` path before the maps are even consulted.

use std::collections::{BTreeMap, HashMap};
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

/// Per-`.bynk`-file line coverage, keyed by a project-relative display path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileCoverage {
    /// Project-relative `.bynk` path (forward-slashed), e.g. `src/limiter.bynk`.
    pub path: String,
    /// Covered executable lines (1-based count).
    pub covered: u32,
    /// Total executable lines attributed to this file (1-based count).
    pub total: u32,
    /// The uncovered executable lines, 1-based and ascending — the exact set the
    /// proposal's fixtures pin.
    pub uncovered: Vec<u32>,
}

/// A whole-run coverage report: one [`FileCoverage`] per measured `.bynk` file,
/// sorted by path, plus the derived totals.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CoverageReport {
    pub files: Vec<FileCoverage>,
}

impl CoverageReport {
    /// Total covered executable lines across every measured file.
    pub fn total_covered(&self) -> u32 {
        self.files.iter().map(|f| f.covered).sum()
    }

    /// Total executable lines across every measured file.
    pub fn total_lines(&self) -> u32 {
        self.files.iter().map(|f| f.total).sum()
    }

    /// Whole-run percentage (0–100), rounded to the nearest integer. A run that
    /// attributed no executable line is reported as 100% (nothing to cover).
    pub fn total_percent(&self) -> u32 {
        percent(self.total_covered(), self.total_lines())
    }

    /// Whether the report attributed no `.bynk` line at all — an empty measured
    /// set (e.g. an integration-only project whose only executed code is the
    /// workers scaffold DECISION D drops).
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

/// Coverage percentage of `covered`/`total`, rounded; `total == 0` → 100.
pub fn percent(covered: u32, total: u32) -> u32 {
    if total == 0 {
        100
    } else {
        ((covered as u64 * 100 + total as u64 / 2) / total as u64) as u32
    }
}

// -- V8 coverage JSON (the `NODE_V8_COVERAGE` output shape) --

#[derive(Deserialize)]
struct V8Document {
    #[serde(default)]
    result: Vec<V8Script>,
}

#[derive(Deserialize)]
struct V8Script {
    url: String,
    #[serde(default)]
    functions: Vec<V8Function>,
}

#[derive(Deserialize)]
struct V8Function {
    #[serde(default)]
    ranges: Vec<V8Range>,
}

#[derive(Deserialize)]
struct V8Range {
    #[serde(rename = "startOffset")]
    start: usize,
    #[serde(rename = "endOffset")]
    end: usize,
    count: i64,
}

/// Collect coverage from a finished run and attribute it to `.bynk` source.
///
/// - `v8_dir` — the directory `NODE_V8_COVERAGE` was pointed at.
/// - `out_js_root` — where the executed `.js` and tsc's `.js.map` live.
/// - `out_root` — where the emitted `.ts` and the emitter's `.ts.map` live.
/// - `source_root` — the project root the `.bynk` paths are relativised against.
///
/// Any file it cannot read or parse is skipped rather than failing the run —
/// coverage is a report *about* a run that already happened, so a partial map
/// should degrade the numbers, never abort. Reading the V8 directory is the one
/// hard error surfaced (it is the runner's own temp dir).
pub fn collect_coverage(
    v8_dir: &Path,
    out_js_root: &Path,
    out_root: &Path,
    source_root: &Path,
) -> std::io::Result<CoverageReport> {
    // 1. Fold every V8 document into per-script merged ranges. A single-process
    //    run writes one file, but Node may split across several; merging the
    //    ranges (and taking the innermost at lookup time) is correct either way.
    let mut scripts: HashMap<PathBuf, Vec<V8Range>> = HashMap::new();
    for entry in std::fs::read_dir(v8_dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(doc) = serde_json::from_str::<V8Document>(&text) else {
            continue;
        };
        for script in doc.result {
            let Some(fs_path) = file_url_to_path(&script.url) else {
                continue;
            };
            let canon = std::fs::canonicalize(&fs_path).unwrap_or(fs_path);
            let ranges = scripts.entry(canon).or_default();
            for func in script.functions {
                ranges.extend(func.ranges);
            }
        }
    }

    let out_js_canon =
        std::fs::canonicalize(out_js_root).unwrap_or_else(|_| out_js_root.to_path_buf());

    // Accumulate per `.bynk` file: the executable lines seen, and which executed.
    let mut acc: BTreeMap<String, FileAcc> = BTreeMap::new();

    for (script_path, ranges) in &scripts {
        let Ok(rel) = script_path.strip_prefix(&out_js_canon) else {
            continue;
        };
        // DECISION D: drop the workers scaffold, the emitted test modules, and
        // the runtime — before the maps are consulted. These are the emitted
        // `out-js` paths, so the filter is on the generated tree, not `.bynk`.
        if !is_measurable_emitted(rel) {
            continue;
        }
        let Ok(js_text) = std::fs::read_to_string(script_path) else {
            continue;
        };
        // Hop 1: tsc's `.js.map` sits beside the `.js`.
        let js_map_path = append_ext(script_path, "map");
        let Some(js_map) = std::fs::read_to_string(&js_map_path)
            .ok()
            .and_then(|s| SourceMap::parse(&s))
        else {
            continue;
        };
        // Hop 2: the emitter's `.ts.map`, mirrored under `out/` at the same rel
        // path (tsc's `rootDir: "."`, `outDir: "../out-js"` keeps the tree 1:1).
        let ts_rel = rel.with_extension("ts");
        let ts_map_path = append_ext(&out_root.join(&ts_rel), "map");
        let Some(ts_map) = std::fs::read_to_string(&ts_map_path)
            .ok()
            .and_then(|s| SourceMap::parse(&s))
        else {
            continue;
        };

        let line_reps = line_representatives(&js_text);
        for (js_line, rep_off) in line_reps.iter().enumerate() {
            let Some(off) = rep_off else { continue };
            // The verdict for this generated line is the *tightest* V8 range
            // covering it — see [`innermost_range`]. A generated line covered by
            // no range (the trailing `sourceMappingURL` comments) attributes
            // nothing.
            let Some((span, count)) = innermost_range(*off, ranges) else {
                continue;
            };
            let Some(js_segs) = js_map.lines.get(js_line) else {
                continue;
            };
            for &(_js_src, ts_line) in js_segs {
                let Some(ts_segs) = ts_map.lines.get(ts_line as usize) else {
                    continue;
                };
                for &(bynk_src, bynk_line) in ts_segs {
                    let Some(bynk_abs) = ts_map.sources.get(bynk_src) else {
                        continue;
                    };
                    let disp = relativise(bynk_abs, source_root);
                    // DECISION D again, on the `.bynk` side: a source that lives
                    // under a `tests`/`workers` component is never user code
                    // under measure (belt-and-suspenders to the `out-js` filter).
                    if has_excluded_component(&disp) {
                        continue;
                    }
                    let file = acc.entry(disp).or_default();
                    // Lines are 1-based in every report the user sees. A `.bynk`
                    // line's verdict is decided by the *most specific* (smallest-
                    // span) generated range mapping to it: a function body's
                    // range beats the whole-module range, so a hoisted
                    // `exports.f = f;` (which runs at load and maps back to the
                    // declaration line) never masks an uncalled function.
                    let line1 = bynk_line + 1;
                    file.observe(line1, span, count);
                }
            }
        }
    }

    let files = acc
        .into_iter()
        .map(|(path, a)| {
            let total = a.lines.len() as u32;
            let mut covered = 0u32;
            let mut uncovered = Vec::new();
            for (&line, &(_, count)) in &a.lines {
                if count > 0 {
                    covered += 1;
                } else {
                    uncovered.push(line);
                }
            }
            FileCoverage {
                path,
                covered,
                total,
                uncovered,
            }
        })
        .collect();
    Ok(CoverageReport { files })
}

#[derive(Default)]
struct FileAcc {
    /// Per 1-based `.bynk` line: the tightest generated range's `(span, count)`
    /// observed for it. `BTreeMap` keeps the uncovered list ascending for free.
    lines: BTreeMap<u32, (usize, i64)>,
}

impl FileAcc {
    /// Record that a generated position inside a range of `span`/`count` maps to
    /// `line`. The tightest span wins; on a span tie the larger count wins (a
    /// position two coverage files agree ran is covered).
    fn observe(&mut self, line: u32, span: usize, count: i64) {
        let slot = self.lines.entry(line).or_insert((usize::MAX, 0));
        if span < slot.0 || (span == slot.0 && count > slot.1) {
            *slot = (span, count);
        }
    }
}

/// The **innermost** (smallest-span) V8 range containing `off`, as `(span,
/// count)`. V8 block coverage nests a not-taken block's `count: 0` range inside
/// its enclosing function's `count: N` range, so the smallest containing range
/// is the precise verdict for that position. `None` if no range contains `off`.
fn innermost_range(off: usize, ranges: &[V8Range]) -> Option<(usize, i64)> {
    let mut best: Option<(usize, i64)> = None;
    for r in ranges {
        if r.start <= off && off < r.end {
            let span = r.end - r.start;
            match best {
                Some((bs, bc)) if span > bs || (span == bs && r.count <= bc) => {}
                _ => best = Some((span, r.count)),
            }
        }
    }
    best
}

/// For each generated line, the byte offset of its first non-whitespace char —
/// the position sampled against the V8 ranges. `None` for a blank line (nothing
/// to attribute; such lines carry no mapping anyway).
fn line_representatives(text: &str) -> Vec<Option<usize>> {
    let mut out = Vec::new();
    let mut off = 0usize;
    for line in text.split_inclusive('\n') {
        let trimmed_lead = line.len() - line.trim_start().len();
        if line.trim().is_empty() {
            out.push(None);
        } else {
            out.push(Some(off + trimmed_lead));
        }
        off += line.len();
    }
    out
}

/// Whether an emitted `out-js`-relative path is a file we measure: not the
/// `tests/` tree, not the `workers/` scaffold, not the shared runtime, and a
/// `.js` module (DECISION D).
fn is_measurable_emitted(rel: &Path) -> bool {
    if rel.extension().and_then(|e| e.to_str()) != Some("js") {
        return false;
    }
    let mut comps = rel.components();
    match comps.next() {
        Some(Component::Normal(c)) if c == "tests" || c == "workers" => return false,
        _ => {}
    }
    // The runtime helpers are framework code, never user source.
    if rel == Path::new("runtime.js") {
        return false;
    }
    true
}

/// Whether a project-relative `.bynk` display path lives under a `tests` or
/// `workers` directory component (DECISION D, on the source side).
fn has_excluded_component(disp: &str) -> bool {
    Path::new(disp)
        .components()
        .any(|c| matches!(c, Component::Normal(s) if s == "tests" || s == "workers"))
}

/// Relativise an absolute `.bynk` source path against the project root, forward-
/// slashed. Falls back to the file name, then the path verbatim, so an
/// out-of-tree source (a synthetic unit) still renders something legible.
fn relativise(abs: &str, source_root: &Path) -> String {
    let p = Path::new(abs);
    if let Ok(rel) = p.strip_prefix(source_root) {
        return forward_slash(rel);
    }
    // The map source is stored forward-slashed absolute; the root may be a
    // different textual form of the same dir. Fall back to the file name.
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| abs.to_string())
}

fn forward_slash(p: &Path) -> String {
    p.components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Convert a `file://` URL to a filesystem path, minimally percent-decoding.
/// Returns `None` for a non-`file:` URL (e.g. `node:internal/...`).
fn file_url_to_path(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    // `file:///abs` → `/abs`; a host part is not expected for local coverage.
    let path = rest
        .strip_prefix('/')
        .map(|r| format!("/{r}"))
        .unwrap_or_else(|| rest.to_string());
    Some(PathBuf::from(percent_decode(&path)))
}

/// Minimal `%XX` percent-decoding — enough for paths with spaces in a temp dir.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Append `.ext` to a path's file name (`foo.js` + `map` → `foo.js.map`), unlike
/// [`Path::with_extension`] which would replace `js`.
fn append_ext(path: &Path, ext: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".");
    name.push(ext);
    path.with_file_name(name)
}

// -- Source-map v3 (the subset needed for line attribution) --

/// A decoded source-map: its `sources` list and, per generated line (0-based),
/// the segments on that line as `(source_index, source_line_0based)`. Generated
/// and source columns are decoded to keep the VLQ deltas honest, then dropped —
/// attribution is line-level on both hops.
struct SourceMap {
    sources: Vec<String>,
    lines: Vec<Vec<(usize, u32)>>,
}

impl SourceMap {
    fn parse(json: &str) -> Option<SourceMap> {
        #[derive(Deserialize)]
        struct Raw {
            #[serde(default)]
            sources: Vec<String>,
            #[serde(default)]
            mappings: String,
        }
        let raw: Raw = serde_json::from_str(json).ok()?;
        Some(SourceMap {
            sources: raw.sources,
            lines: decode_line_mappings(&raw.mappings),
        })
    }
}

/// Decode a v3 `mappings` string into, per generated line, its `(src_idx,
/// src_line)` segments (both 0-based). Follows the VLQ delta rules: the source
/// index/line/column deltas persist across segments *and* lines; the generated
/// column resets at each line boundary. A one-field segment (generated column
/// only, no source) carries no attribution and is skipped.
fn decode_line_mappings(mappings: &str) -> Vec<Vec<(usize, u32)>> {
    let mut lines = Vec::new();
    // Source index and source line are independent running totals that persist
    // across segments and lines (the source column would be a third, but line
    // attribution never needs it). The generated column resets each line and is
    // irrelevant here, so it is decoded but discarded.
    let (mut src, mut src_line) = (0i64, 0i64);
    for seg_line in mappings.split(';') {
        let mut segs = Vec::new();
        for seg in seg_line.split(',') {
            if seg.is_empty() {
                continue;
            }
            let fields = vlq_decode(seg);
            if fields.len() >= 4 {
                src += fields[1];
                src_line += fields[2];
                if src >= 0 && src_line >= 0 {
                    segs.push((src as usize, src_line as u32));
                }
            }
        }
        lines.push(segs);
    }
    lines
}

/// Base64-VLQ-decode one segment into its signed fields.
fn vlq_decode(seg: &str) -> Vec<i64> {
    const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let (mut shift, mut acc) = (0u32, 0i64);
    for &c in seg.as_bytes() {
        let Some(d) = B64.iter().position(|&b| b == c).map(|p| p as i64) else {
            continue;
        };
        acc += (d & 0b11111) << shift;
        if d & 0b100000 != 0 {
            shift += 5;
        } else {
            let value = if acc & 1 == 1 { -(acc >> 1) } else { acc >> 1 };
            out.push(value);
            shift = 0;
            acc = 0;
        }
    }
    out
}

// -- Rich rendering --

/// Render the human coverage summary — a per-file bar, percentage, covered/total
/// line count, and the uncovered-line ranges — plus a total row. Trailing
/// newline. An empty report renders a single "no coverage attributed" note.
pub fn render_rich(report: &CoverageReport) -> String {
    let mut out = String::new();
    out.push_str("\nCoverage\n");
    if report.is_empty() {
        out.push_str("  (no executable `.bynk` lines were attributed)\n");
        return out;
    }
    let name_w = report
        .files
        .iter()
        .map(|f| f.path.len())
        .max()
        .unwrap_or(0)
        .max(5);
    for f in &report.files {
        let pct = percent(f.covered, f.total);
        out.push_str(&format!(
            "  {:<name_w$}  {}  {:>3}%   ({}/{} lines)",
            f.path,
            bar(f.covered, f.total),
            pct,
            f.covered,
            f.total,
        ));
        if !f.uncovered.is_empty() {
            out.push_str(&format!("  uncovered: {}", format_ranges(&f.uncovered)));
        }
        out.push('\n');
    }
    let dashes = "─".repeat(name_w + 24);
    out.push_str(&format!("  {dashes}\n"));
    out.push_str(&format!(
        "  {:<name_w$}  {}  {:>3}%   ({}/{} lines)\n",
        "total",
        bar(report.total_covered(), report.total_lines()),
        report.total_percent(),
        report.total_covered(),
        report.total_lines(),
    ));
    out
}

/// An 8-cell coverage bar: filled `▓` proportional to the covered fraction, the
/// remainder `·`.
fn bar(covered: u32, total: u32) -> String {
    const CELLS: u32 = 8;
    let filled = if total == 0 {
        CELLS
    } else {
        (covered as u64 * CELLS as u64 / total as u64) as u32
    };
    let mut s = String::new();
    for _ in 0..filled {
        s.push('▓');
    }
    for _ in filled..CELLS {
        s.push('·');
    }
    s
}

/// Compress a sorted line list into comma-separated ranges: `[6,7,8,51]` →
/// `"6-8, 51"`.
pub fn format_ranges(lines: &[u32]) -> String {
    let mut parts = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let start = lines[i];
        let mut end = start;
        while i + 1 < lines.len() && lines[i + 1] == end + 1 {
            end += 1;
            i += 1;
        }
        if start == end {
            parts.push(format!("{start}"));
        } else {
            parts.push(format!("{start}-{end}"));
        }
        i += 1;
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vlq_decodes_signed_fields() {
        // "AAAA" → [0,0,0,0]; "AAEA" → [0,0,2,0]; "AACE" → [0,0,1,2]; "D" → [-1].
        assert_eq!(vlq_decode("AAAA"), vec![0, 0, 0, 0]);
        assert_eq!(vlq_decode("AAEA"), vec![0, 0, 2, 0]);
        assert_eq!(vlq_decode("AACE"), vec![0, 0, 1, 2]);
        assert_eq!(vlq_decode("D"), vec![-1]);
    }

    #[test]
    fn line_mappings_track_running_source_line() {
        // The emitter's `.ts.map` for the two-fn spike: 5 blank lines, then
        // src lines 2,3,3,6,7,8 (0-based) — running deltas across lines.
        let m = ";;;;;AAEA;AACE;AAAA;AAGF;AACE;AACA";
        let lines = decode_line_mappings(m);
        assert_eq!(lines[0], vec![]);
        assert_eq!(lines[5], vec![(0, 2)]);
        assert_eq!(lines[6], vec![(0, 3)]);
        assert_eq!(lines[7], vec![(0, 3)]);
        assert_eq!(lines[8], vec![(0, 6)]);
        assert_eq!(lines[9], vec![(0, 7)]);
        assert_eq!(lines[10], vec![(0, 8)]);
    }

    #[test]
    fn innermost_range_prefers_the_tightest_span() {
        // A module range (count 1) with a nested uncalled-fn range (count 0):
        // a position inside the fn is uncovered, one outside it is covered.
        let ranges = vec![
            V8Range {
                start: 0,
                end: 100,
                count: 1,
            },
            V8Range {
                start: 40,
                end: 60,
                count: 0,
            },
        ];
        assert_eq!(innermost_range(10, &ranges), Some((100, 1)));
        assert_eq!(innermost_range(50, &ranges), Some((20, 0)));
        assert_eq!(innermost_range(200, &ranges), None); // outside every range
    }

    #[test]
    fn file_acc_lets_the_tightest_range_decide() {
        // The hoisted-export hazard: a `.bynk` line reached both by a wide
        // module range that ran (the `exports.f = f;` line) and a tight function
        // range that did not (the never-called body). The tight range wins →
        // uncovered, not falsely covered.
        let mut acc = FileAcc::default();
        acc.observe(7, 352, 1); // module-level hoisted export, executed
        acc.observe(7, 61, 0); // the function body's own range, never run
        assert_eq!(acc.lines.get(&7), Some(&(61, 0)));
    }

    #[test]
    fn line_representatives_pick_first_nonspace() {
        let reps = line_representatives("ab\n    cd\n\n  \nx");
        assert_eq!(reps[0], Some(0)); // "ab"
        assert_eq!(reps[1], Some(7)); // "    cd" → 'c' at 3+4
        assert_eq!(reps[2], None); // blank
        assert_eq!(reps[3], None); // whitespace-only
        assert_eq!(reps[4], Some(14)); // "x"
    }

    #[test]
    fn format_ranges_compresses_runs() {
        assert_eq!(format_ranges(&[6, 7, 8, 51]), "6-8, 51");
        assert_eq!(format_ranges(&[3]), "3");
        assert_eq!(format_ranges(&[1, 2, 4, 5, 6]), "1-2, 4-6");
        assert_eq!(format_ranges(&[]), "");
    }

    #[test]
    fn percent_rounds_and_guards_zero() {
        assert_eq!(percent(0, 0), 100);
        assert_eq!(percent(42, 49), 86);
        assert_eq!(percent(2, 5), 40);
        assert_eq!(percent(11, 11), 100);
    }

    #[test]
    fn measurable_filter_drops_tests_workers_runtime() {
        assert!(is_measurable_emitted(Path::new("src/limiter.js")));
        assert!(is_measurable_emitted(Path::new("limiter.js")));
        assert!(!is_measurable_emitted(Path::new("tests/main.js")));
        assert!(!is_measurable_emitted(Path::new("workers/api/handlers.js")));
        assert!(!is_measurable_emitted(Path::new("runtime.js")));
        assert!(!is_measurable_emitted(Path::new("src/limiter.js.map")));
    }

    #[test]
    fn file_url_round_trips_to_path() {
        assert_eq!(
            file_url_to_path("file:///private/tmp/a%20b/out-js/m.js"),
            Some(PathBuf::from("/private/tmp/a b/out-js/m.js"))
        );
        assert_eq!(file_url_to_path("node:internal/modules"), None);
    }
}
