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

    let out = bynkc::compile_project(&bynk_testkit::compile_options_single(src.clone()))
        .map_err(bynkc::ProjectFailure::flatten)
        .unwrap_or_else(|e| panic!("compile failed: {e:?}"));

    let main_path = Path::new("reps.ts");
    let ts = out
        .artefacts
        .docs
        .get(main_path)
        .expect("reps.ts in output")
        .text();
    let map = out
        .artefacts
        .docs
        .get(&bynkc::sibling_path(main_path, "map"))
        .expect("reps.ts carries a source map")
        .text();
    let _ = std::fs::remove_dir_all(&dir);
    (ts, map)
}

/// #1352: like [`compile_reps`], but with the dev/test contract call-site
/// guard on (`CompileOptions::contracts(true)`, DECISION J — off by default,
/// `compile_options_single`'s own `contracts: false`) — the one real branch
/// `emit_free_fn`'s own source-map fix (#1352) left untested by every other
/// test in this file: `emit_contract_guarded_body` lowers its own body into
/// the SAME `body_text`/`merge` machinery, nested one IIFE deeper.
fn compile_reps_with_contracts(source: &str) -> (String, String) {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "bynk_srcmap_contracts_{}_{unique}",
        std::process::id()
    ));
    let src = dir.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("reps.bynk"), source).unwrap();

    let out =
        bynkc::compile_project(&bynk_testkit::compile_options_single(src.clone()).contracts(true))
            .map_err(bynkc::ProjectFailure::flatten)
            .unwrap_or_else(|e| panic!("compile failed: {e:?}"));

    let main_path = Path::new("reps.ts");
    let ts = out
        .artefacts
        .docs
        .get(main_path)
        .expect("reps.ts in output")
        .text();
    let map = out
        .artefacts
        .docs
        .get(&bynkc::sibling_path(main_path, "map"))
        .expect("reps.ts carries a source map")
        .text();
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
    // v0.72: the map `source` is the file's absolute path (forward-slashed), so an
    // editor breakpoint on the real `.bynk` binds — hence a suffix check, not an
    // exact relative match.
    assert!(
        map.contains("/reps.bynk\"]"),
        "sources ends in /reps.bynk: {map}"
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

const IIFE_FIXTURE: &str = "commons reps {
  fn compute(cond: Bool) -> Int {
    let a = 1
    let r = if cond {
      let y = 2
      y + 1
    } else {
      0
    }
    a + r
  }
}
";

#[test]
fn value_position_if_iife_does_not_corrupt_earlier_checkpoints() {
    // #4 review: `lower_if`'s value-position IIFE builds its `then`/`else`
    // branches into a local buffer (`iife`), spliced into the real output
    // only once fully built. Before `without_source_map`, `record_span` was
    // still called for statements inside that local buffer (e.g. `let y = 2`
    // / `y + 1` / the `else` arm's `0`) with an offset relative to *that*
    // buffer's own (small) length — but `to_v3` resolves every checkpoint's
    // generated line against the *final*, fully-emitted file's line table.
    // A small IIFE-local offset (tens of bytes) resolves there to whatever
    // line actually sits at that byte position in the real file — here, the
    // header comment / import lines, nowhere near the `if`. Confirmed by
    // reverting the fix locally: the header lines (0-4) came back mapped to
    // source lines 5 and 7 (`y + 1` and the `else` arm's `0`) instead of
    // staying unmapped, silently overriding whatever checkpoint correctly
    // belonged to those early lines. With recording suppressed for the
    // duration of the local buffer, they correctly stay unmapped instead.
    let (ts, map) = compile_reps(IIFE_FIXTURE);
    let lines = decode(extract_field(&map, "mappings"));

    let header_end = gen_line_of(&ts, "export function compute");
    for (g, mapped) in lines.iter().enumerate().take(header_end) {
        assert_eq!(
            *mapped, None,
            "gen line {g} (before the function starts) must not be mapped to \
             a source line stolen from inside the IIFE:\n{ts}"
        );
    }

    let at = |g: usize| lines[g].unwrap_or_else(|| panic!("gen line {g} unmapped"));
    // Source (0-based): line 2 = `let a = 1`, line 9 = `a + r`.
    assert_eq!(
        at(gen_line_of(&ts, "const a = ")),
        2,
        "`let a` keeps its own line"
    );
    assert_eq!(
        at(gen_line_of(&ts, "return a + r")),
        9,
        "the tail `a + r` keeps its own line"
    );
}

/// #1359: like [`compile_reps`], but for a `context`-rooted fixture (a
/// `capability`/`provides` pair can't live in a bare `commons` at all —
/// `emit_capability`'s own #1358 test hit the identical constraint) —
/// `compile_options_single` still discovers it fine, the file just needs to
/// be named after the CONTEXT, not a `commons`.
fn compile_reps_context(source: &str) -> (String, String) {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "bynk_srcmap_context_{}_{unique}",
        std::process::id()
    ));
    let src = dir.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("reps.bynk"), source).unwrap();

    let out = bynkc::compile_project(&bynk_testkit::compile_options_single(src.clone()))
        .map_err(bynkc::ProjectFailure::flatten)
        .unwrap_or_else(|e| panic!("compile failed: {e:?}"));

    let main_path = Path::new("reps.ts");
    let ts = out
        .artefacts
        .docs
        .get(main_path)
        .expect("reps.ts in output")
        .text();
    let map = out
        .artefacts
        .docs
        .get(&bynkc::sibling_path(main_path, "map"))
        .expect("reps.ts carries a source map")
        .text();
    let _ = std::fs::remove_dir_all(&dir);
    (ts, map)
}

const PROVIDER_FIXTURE: &str = "context reps {
  capability Calc {
    fn double(n: Int) -> Int
  }

  provides Calc = SimpleCalc {
    fn double(n: Int) -> Int {
      let doubled = n * 2
      doubled
    }
  }
}
";

#[test]
fn provider_op_body_keeps_its_own_statement_lines() {
    // #1359: emit_provider's own per-op method bodies now go through the
    // same local-sub-builder-then-merge pattern #1352/#1353 established for
    // emit_free_fn/emit_contract_guarded_body -- applied once per method
    // instead of once per function, since the whole class's own wrapper
    // stays hand-written but each method is a real, individually-printed
    // TsClassMethod fragment. Regressing to a direct-into-out (or a
    // wrongly-based local buffer) lowering would collapse `double`'s own
    // body statements toward whatever offset the buffer's own length
    // happened to land on -- the same failure mode
    // `value_position_if_iife_does_not_corrupt_earlier_checkpoints`/
    // `contract_guarded_body_keeps_its_own_statement_lines` already pin for
    // the sibling cases.
    let (ts, map) = compile_reps_context(PROVIDER_FIXTURE);
    let lines = decode(extract_field(&map, "mappings"));
    let at = |g: usize| lines[g].unwrap_or_else(|| panic!("gen line {g} unmapped\n{ts}"));

    // Source (0-based): line 0 = `context reps {`, line 5 = `provides Calc = SimpleCalc {`,
    // line 7 = `let doubled = n * 2`, line 8 = `doubled`.
    assert_eq!(
        at(gen_line_of(&ts, "class SimpleCalc")),
        5,
        "the class header -> `provides Calc = SimpleCalc` source line"
    );
    assert_eq!(
        at(gen_line_of(&ts, "const doubled = ")),
        7,
        "`let doubled` keeps its own line, not collapsed toward the class header"
    );
    assert_eq!(
        at(gen_line_of(&ts, "return doubled;")),
        8,
        "the tail `doubled` keeps its own line"
    );
}

const CONTRACT_FIXTURE: &str = "commons reps {
  fn scale(n: Int) -> Int
  requires positive: n > 0
  ensures small: result < 1000
  {
    let doubled = n * 2
    doubled
  }
}
";

#[test]
fn contract_guarded_body_keeps_its_own_statement_lines() {
    // #1352: emit_contract_guarded_body's own body -- the branch emit_free_fn's
    // source-map fix must also cover -- lowers through the SAME body_smb/merge
    // machinery as the ordinary (unguarded) path, one IIFE deeper. Regressing
    // to the pre-#1352 direct-into-out lowering collapses every statement
    // inside `scale`'s own body toward whatever offset the isolated buffer's
    // own (small) length happened to land on in the real file -- exactly the
    // failure mode `value_position_if_iife_does_not_corrupt_earlier_checkpoints`
    // above already pins for the unguarded IIFE case, here for the guarded one.
    let (ts, map) = compile_reps_with_contracts(CONTRACT_FIXTURE);
    let lines = decode(extract_field(&map, "mappings"));
    let at = |g: usize| lines[g].unwrap_or_else(|| panic!("gen line {g} unmapped\n{ts}"));

    // Source (0-based): line 0 = `commons reps {`, line 1 = `fn scale(...)`,
    // line 5 = `let doubled = n * 2`, line 6 = `doubled`.
    assert_eq!(
        at(gen_line_of(&ts, "contract violated: precondition")),
        1,
        "precondition guard -> `fn scale` header line"
    );
    assert_eq!(
        at(gen_line_of(&ts, "const doubled = ")),
        5,
        "`let doubled` keeps its own line, not collapsed toward the module header"
    );
    assert_eq!(
        at(gen_line_of(&ts, "return doubled;")),
        6,
        "the tail `doubled` keeps its own line"
    );
}
