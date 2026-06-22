//! Decode goldens for v0.70 — source maps for spliced bodies (handlers + tests).
//!
//! Slice 1 mapped free-function bodies and declarations; the bodies that lower
//! through a *spliced local buffer* (service/agent handlers, test cases) stayed at
//! declaration granularity. These tests confirm those bodies now map per-statement,
//! by compiling a real project, decoding the emitted `.ts.map`, and asserting the
//! named `.bynk` lines — including a multi-file test group, to exercise the
//! multi-source map.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

fn tmp(tag: &str) -> PathBuf {
    static C: AtomicU32 = AtomicU32::new(0);
    std::env::temp_dir().join(format!(
        "bynk_smb_{tag}_{}_{}",
        std::process::id(),
        C.fetch_add(1, Ordering::Relaxed)
    ))
}

/// Decode a (possibly multi-source) v3 mappings string into, per generated line,
/// `Some((source_id, source_line))` — both 0-based — for the one-segment-per-line
/// maps the emitter produces.
fn decode(mappings: &str) -> Vec<Option<(i64, i64)>> {
    const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let dec = |s: &str| -> Vec<i64> {
        let mut out = Vec::new();
        let (mut shift, mut acc) = (0i64, 0i64);
        for &c in s.as_bytes() {
            let d = B64.iter().position(|&b| b == c).unwrap() as i64;
            acc += (d & 0b11111) << shift;
            if d & 0b100000 != 0 {
                shift += 5;
            } else {
                out.push(if acc & 1 == 1 { -(acc >> 1) } else { acc >> 1 });
                shift = 0;
                acc = 0;
            }
        }
        out
    };
    let (mut src, mut sl) = (0i64, 0i64);
    let mut lines = Vec::new();
    for seg in mappings.split(';') {
        if seg.is_empty() {
            lines.push(None);
            continue;
        }
        let f = dec(seg); // [genCol, srcIdxDelta, srcLineDelta, srcCol]
        src += f[1];
        sl += f[2];
        lines.push(Some((src, sl)));
    }
    lines
}

/// `(sources, mappings)` from a v3 JSON document (hand-extracted; no serde dep).
fn parse_map(json: &str) -> (Vec<String>, String) {
    let sources = {
        let k = "\"sources\":[";
        let start = json.find(k).unwrap() + k.len();
        let rest = &json[start..];
        let end = rest.find(']').unwrap();
        rest[..end]
            .split(',')
            .map(|s| s.trim().trim_matches('"').to_string())
            .collect()
    };
    let mappings = {
        let k = "\"mappings\":\"";
        let start = json.find(k).unwrap() + k.len();
        let rest = &json[start..];
        rest[..rest.find('"').unwrap()].to_string()
    };
    (sources, mappings)
}

fn gen_line_of(ts: &str, needle: &str) -> usize {
    ts.lines()
        .position(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no generated line contains {needle:?}\n{ts}"))
}

/// Compile a project (optionally Workers target) and return the `(typescript,
/// source_map)` of the one output file whose path ends with `suffix`.
fn compile_file(dir: &Path, workers: bool, suffix: &str) -> (String, String) {
    let opts = bynkc::CompileOptions::split(dir.to_path_buf(), bynkc::read_project_paths(dir));
    let opts = if workers {
        opts.target(bynkc::BuildTarget::Workers)
    } else {
        opts
    };
    let out = bynkc::compile_project(&opts)
        .map_err(bynkc::ProjectFailure::flatten)
        .unwrap_or_else(|e| panic!("compile failed: {e:?}"));
    let f = out
        .files
        .iter()
        .find(|f| {
            f.output_path
                .to_string_lossy()
                .replace('\\', "/")
                .ends_with(suffix)
        })
        .unwrap_or_else(|| panic!("no output file ending {suffix:?}"));
    let map = f
        .source_map
        .clone()
        .unwrap_or_else(|| panic!("{suffix} carries no source map"));
    (f.typescript.clone(), map)
}

#[test]
fn service_handler_body_maps_per_statement() {
    let dir = tmp("svc");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("bynk.toml"), "[project]\nname = \"svc\"\n").unwrap();
    // Handler body on .bynk lines 7 (the effect-let) and 8 (the Ok tail).
    std::fs::write(
        dir.join("src").join("svc.bynk"),
        "context svc\n\nconsumes bynk { Logger }\n\nservice api from http {\n\ton GET(\"/\") by v: Visitor () -> Effect[HttpResult[String]] given Logger {\n\t\tlet _ <- Logger.info(\"hi\")\n\t\tOk(\"hello\")\n\t}\n}\n",
    )
    .unwrap();

    let (ts, map) = compile_file(&dir, true, "handlers.ts");
    let _ = std::fs::remove_dir_all(&dir);
    let (sources, mappings) = parse_map(&map);
    assert_eq!(sources, vec!["svc.bynk"]);
    let lines = decode(&mappings);
    let at = |g: usize| {
        lines[g]
            .unwrap_or_else(|| panic!("gen line {g} unmapped"))
            .1
    };

    // The lowered effect-let maps to .bynk line 7 (0-based 6), not the `service`
    // declaration line — the whole point of v0.70.
    assert_eq!(
        at(gen_line_of(&ts, "await deps.Logger.info")),
        6,
        "effect-let -> .bynk:7"
    );
    assert_eq!(
        at(gen_line_of(&ts, "return HttpResult.Ok")),
        7,
        "Ok tail -> .bynk:8"
    );
    // The `service`/handler signature still anchors to the declaration line (4, the
    // `service` keyword line, 0-based).
    assert_eq!(
        at(gen_line_of(&ts, "export const api")),
        4,
        "signature -> service decl"
    );
}

#[test]
fn unit_test_body_maps_to_its_bynk_source() {
    let dir = tmp("tb");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("tests")).unwrap();
    std::fs::write(dir.join("bynk.toml"), "[project]\nname = \"tb\"\n").unwrap();
    std::fs::write(
        dir.join("src").join("calc.bynk"),
        "commons calc {\n  fn dbl(n: Int) -> Int {\n    n + n\n  }\n}\n",
    )
    .unwrap();
    // Test body: `let r = dbl(3)` on .bynk line 3, `assert r == 6` on line 4.
    std::fs::write(
        dir.join("tests").join("calc.bynk"),
        "test calc {\n  test \"doubles\" {\n    let r = dbl(3)\n    assert r == 6\n  }\n}\n",
    )
    .unwrap();

    let (ts, map) = compile_file(&dir, false, "tests/calc.test.ts");
    let _ = std::fs::remove_dir_all(&dir);
    let (sources, mappings) = parse_map(&map);
    assert_eq!(sources, vec!["tests/calc.bynk"]);
    let lines = decode(&mappings);
    let at = |g: usize| {
        lines[g]
            .unwrap_or_else(|| panic!("gen line {g} unmapped"))
            .1
    };

    assert_eq!(
        at(gen_line_of(&ts, "const r = dbl(3)")),
        2,
        "let -> .bynk:3"
    );
    // `r === 6` is unique to the assert *statement* (the `__bynkAssertionFailure`
    // helper is also defined earlier in the module).
    assert_eq!(at(gen_line_of(&ts, "r === 6")), 3, "assert -> .bynk:4");
}

#[test]
fn multi_file_test_group_has_multiple_sources() {
    // One `test calc` target split across two `.bynk` files → the test module map
    // must carry both as sources, and each case map to its own file.
    let dir = tmp("multi");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("tests").join("calc")).unwrap();
    std::fs::write(dir.join("bynk.toml"), "[project]\nname = \"multi\"\n").unwrap();
    std::fs::write(
        dir.join("src").join("calc.bynk"),
        "commons calc {\n  fn dbl(n: Int) -> Int {\n    n + n\n  }\n}\n",
    )
    .unwrap();
    // A directory test group: two files sharing the `test calc` header.
    std::fs::write(
        dir.join("tests").join("calc").join("a.bynk"),
        "test calc {\n  test \"a\" {\n    assert dbl(1) == 2\n  }\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("tests").join("calc").join("b.bynk"),
        "test calc {\n  test \"b\" {\n    assert dbl(2) == 4\n  }\n}\n",
    )
    .unwrap();

    let (_ts, map) = compile_file(&dir, false, "tests/calc.test.ts");
    let _ = std::fs::remove_dir_all(&dir);
    let (sources, mappings) = parse_map(&map);
    assert!(
        sources.len() == 2
            && sources.iter().any(|s| s.ends_with("a.bynk"))
            && sources.iter().any(|s| s.ends_with("b.bynk")),
        "module map should carry both test files as sources, got {sources:?}"
    );
    // Both source ids appear in the mappings (each case maps to its own file).
    let used: std::collections::HashSet<i64> = decode(&mappings)
        .into_iter()
        .flatten()
        .map(|(s, _)| s)
        .collect();
    assert!(
        used.contains(&0) && used.contains(&1),
        "both sources referenced, got {used:?}"
    );
}
