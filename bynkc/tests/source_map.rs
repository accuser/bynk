//! Source-map decode goldens (debugging track, slice 1; ADR 0103).
//!
//! Rather than commit opaque VLQ blobs, these tests compile a fixture, decode
//! the emitted `.ts.map`, and assert the *named* source→generated line pairs the
//! slice-0 spike fixed — the only golden form a reviewer can actually read. The
//! load-bearing claim is ADR 0103 D2 (nearest-enclosing statement): the `?`
//! `Err`-guard and the `match` `case` lines map back to their enclosing
//! statement, so a source-map-aware stepper coalesces the lowered expansion.

use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

/// Compile a single-commons fixture and return `(generated_ts, source_map_json)`
/// for its `reps.ts`. Each call uses its own temp dir — the tests run in parallel,
/// so a shared dir would race (one test's cleanup deletes another's input).
fn compile_reps(source: &str) -> (String, String) {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("bynk_srcmap_{}_{unique}", std::process::id()));
    let src = dir.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("reps.bynk"), source).unwrap();

    let out = bynkc::compile_project(&bynkc::CompileOptions::single(src.clone()))
        .map_err(bynkc::ProjectFailure::flatten)
        .unwrap_or_else(|e| panic!("compile failed: {e:?}"));

    let file = out
        .files
        .iter()
        .find(|f| f.output_path.as_path() == Path::new("reps.ts"))
        .expect("reps.ts in output");
    let map = file
        .source_map
        .clone()
        .expect("reps.ts carries a source map");
    let ts = file.typescript.clone();
    let _ = std::fs::remove_dir_all(&dir);
    (ts, map)
}

/// Decode the `mappings` string into `gen_line0 -> Some(src_line0)`, for the
/// one-segment-per-line maps the builder emits.
fn decode(mappings: &str) -> Vec<Option<i64>> {
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
    let mut src_line = 0i64;
    let mut lines = Vec::new();
    for seg in mappings.split(';') {
        if seg.is_empty() {
            lines.push(None);
            continue;
        }
        src_line += dec(seg)[2]; // [genCol, srcIdx, srcLineDelta, srcCol]
        lines.push(Some(src_line));
    }
    lines
}

fn extract_field<'a>(json: &'a str, key: &str) -> &'a str {
    let k = format!("\"{key}\":\"");
    let start = json.find(&k).expect("key present") + k.len();
    let rest = &json[start..];
    &rest[..rest.find('"').unwrap()]
}

/// The generated line (0-based) of the first line containing `needle`.
fn gen_line_of(ts: &str, needle: &str) -> usize {
    ts.lines()
        .position(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no generated line contains {needle:?}\n{ts}"))
}

/// Every generated line containing `needle` (0-based), in order.
fn gen_lines_of(ts: &str, needle: &str) -> Vec<usize> {
    ts.lines()
        .enumerate()
        .filter(|(_, l)| l.contains(needle))
        .map(|(i, _)| i)
        .collect()
}

const FIXTURE: &str = "commons reps {
  type Reps = Int where InRange(1, 100)

  fn total(warmup: Int, working: Int) -> Result[Int, ValidationError] {
    let w = Reps.of(warmup)?
    let k = Reps.of(working)?
    Ok(w + k)
  }

  fn describe(warmup: Int, working: Int) -> String {
    let outcome = total(warmup, working)
    match outcome {
      Ok(n) => \"valid plan\"
      Err(e) => \"invalid plan\"
    }
  }
}
";

#[test]
fn map_is_valid_v3_with_embedded_source() {
    let (_ts, map) = compile_reps(FIXTURE);
    assert!(map.contains("\"version\":3"), "v3 header: {map}");
    assert!(
        map.contains("\"sources\":[\"reps.bynk\"]"),
        "sources: {map}"
    );
    // sourcesContent embeds the .bynk for dev/test fidelity (ADR 0103 D6). The
    // fixture line has no quotes, so it appears verbatim inside the JSON array.
    assert!(
        map.contains("\"sourcesContent\":["),
        "has sourcesContent array"
    );
    assert!(
        map.contains("let w = Reps.of(warmup)?"),
        "sourcesContent embeds the .bynk source"
    );
}

#[test]
fn question_propagation_anchors_to_its_let_statement() {
    // ADR 0103 D2: the `?` lowers to temp / Err-guard / unwrap — all three
    // generated lines must map back to the single `let` source line, so stepping
    // sees one source step, not a phantom stop on the guard. (Spike: 8→3.)
    let (ts, map) = compile_reps(FIXTURE);
    let lines = decode(extract_field(&map, "mappings"));
    let at = |g: usize| lines[g].unwrap_or_else(|| panic!("gen line {g} unmapped"));

    // Source (0-based): line 4 = `let w = Reps.of(warmup)?`, line 5 = `let k`.
    let guards = gen_lines_of(&ts, ".tag === \"Err\") return"); // both `?` guards
    assert_eq!(guards.len(), 2, "two `?` guards");
    assert_eq!(at(guards[0]), 4, "first `?` guard → `let w` source line");
    assert_eq!(at(guards[1]), 5, "second `?` guard → `let k` source line");

    // The unwrap binding shares the same source line as its guard.
    assert_eq!(at(gen_line_of(&ts, "const w = ")), 4);
    assert_eq!(at(gen_line_of(&ts, "const k = ")), 5);

    // The tail `Ok(w + k)` is source line 6. (Needle is specific: `return Ok(`
    // alone also matches the `Reps.of()` constructor's `return Ok(value …)`.)
    assert_eq!(at(gen_line_of(&ts, "return Ok(w + k)")), 6);
}

#[test]
fn match_arms_anchor_to_their_arm_source_line() {
    // ADR 0103 D2: each `match` arm's `case`/binding/`return` maps to that arm's
    // source line, so stepping a match walks arm-to-arm. (Spike: 13→6.)
    let (ts, map) = compile_reps(FIXTURE);
    let lines = decode(extract_field(&map, "mappings"));
    let at = |g: usize| lines[g].unwrap_or_else(|| panic!("gen line {g} unmapped"));

    // Source (0-based): 11 = `match outcome {`, 12 = `Ok(n) => …`, 13 = `Err(e) => …`.
    assert_eq!(
        at(gen_line_of(&ts, "switch (outcome.tag)")),
        11,
        "switch → match head"
    );
    assert_eq!(at(gen_line_of(&ts, "case \"Ok\":")), 12, "Ok case → Ok arm");
    assert_eq!(
        at(gen_line_of(&ts, "case \"Err\":")),
        13,
        "Err case → Err arm"
    );
}

#[test]
fn declarations_anchor_to_their_declaration() {
    // Signature lines map to the declaration's span (so a breakpoint on `fn`
    // binds at the function header).
    let (ts, map) = compile_reps(FIXTURE);
    let lines = decode(extract_field(&map, "mappings"));
    let at = |g: usize| lines[g].unwrap_or_else(|| panic!("gen line {g} unmapped"));

    assert_eq!(
        at(gen_line_of(&ts, "export function total")),
        3,
        "total → `fn total`"
    );
    assert_eq!(
        at(gen_line_of(&ts, "export function describe")),
        9,
        "describe → `fn describe`"
    );
}
