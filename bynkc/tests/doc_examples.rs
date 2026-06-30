//! The example-compilation gate.
//!
//! Extracts every fenced ```bynk block from the Book
//! (`site/src/content/docs/book/**`) plus the landing page
//! (`site/src/content/docs/index.mdx`, which ships the most visible example) and
//! compiles each, so a doc example can never fall out of step with the compiler.
//!
//! Block handling, by the fence's info string and the block's first line:
//! - ```bynk,ignore  → skipped (fragments, pseudo-syntax, partial snippets).
//! - ```bynk,fail    → must FAIL to compile (negative examples).
//! - ```bynk         → checked:
//!     * a block starting `commons …` is compiled as a single file;
//!     * a block starting `context …` is compiled as a one-file project;
//!     * anything else (a fragment, a `test` block, a bare expression) is
//!       skipped — it cannot stand alone.
//!
//! Skip counts are printed so unchecked blocks are never silently assumed good.
//! Run with `--nocapture` to see the summary.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
struct Block {
    file: String,
    line: usize,
    info: String,
    body: String,
}

fn docs_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../site/src/content/docs/book")
}

fn docs_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../site/src/content/docs")
}

fn landing_page() -> PathBuf {
    docs_root().join("index.mdx")
}

fn collect_blocks() -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut files = Vec::new();
    gather_md(&docs_src(), &mut files);
    files.sort();
    // The landing page lives outside the Book but ships the most visible bynk
    // example; gate it too so the front door can't drift from the compiler.
    files.push(landing_page());
    for file in files {
        let text = fs::read_to_string(&file).unwrap();
        let rel = file
            .strip_prefix(docs_root())
            .unwrap_or(&file)
            .display()
            .to_string();
        let mut lines = text.lines().enumerate();
        while let Some((idx, line)) = lines.next() {
            let trimmed = line.trim_start();
            if let Some(rest) = trimmed.strip_prefix("```bynk") {
                // `rest` is "" for ```bynk, or ",fail" / ",ignore" etc.
                let info = rest.trim_start_matches(',').to_string();
                let mut body = String::new();
                for (_, l) in lines.by_ref() {
                    if l.trim() == "```" {
                        break;
                    }
                    body.push_str(l);
                    body.push('\n');
                }
                blocks.push(Block {
                    file: rel.clone(),
                    line: idx + 1,
                    info,
                    body,
                });
            }
        }
    }
    blocks
}

fn gather_md(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            gather_md(&path, out);
        } else if path.extension().is_some_and(|e| e == "md") {
            out.push(path);
        }
    }
}

fn first_line(body: &str) -> &str {
    body.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
}

/// Compile a `context …` block as a one-file project under a unique temp dir.
fn compile_context(body: &str, idx: usize) -> Result<(), String> {
    let first = first_line(body);
    let name = first
        .strip_prefix("context")
        .unwrap_or("")
        .trim()
        .trim_end_matches('{')
        .trim();
    if name.is_empty() {
        return Err("could not parse context name".to_string());
    }
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("doc-ctx-{idx}"));
    let _ = fs::remove_dir_all(&root);
    let rel: PathBuf = name.split('.').collect::<PathBuf>().with_extension("bynk");
    let file = root.join(&rel);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, body).unwrap();

    let result = bynkc::compile_project(&bynkc::CompileOptions::single(root.clone()))
        .map_err(bynkc::ProjectFailure::flatten)
        .map(|_| ())
        .map_err(|errs| {
            errs.iter()
                .map(|e| format!("{}: {}", e.category, e.message))
                .collect::<Vec<_>>()
                .join("; ")
        });
    let _ = fs::remove_dir_all(&root);
    result
}

#[test]
fn every_doc_example_compiles() {
    let blocks = collect_blocks();
    assert!(!blocks.is_empty(), "found no ```bynk blocks under the Book");

    let (mut checked_ok, mut checked_fail, mut skip_ignored, mut skip_fragment, mut skip_include) =
        (0, 0, 0, 0, 0);
    let mut failures: Vec<String> = Vec::new();

    for (idx, b) in blocks.iter().enumerate() {
        let loc = format!("{}:{} (`{}…`)", b.file, b.line, first_line(&b.body));

        if b.info.contains("ignore") {
            skip_ignored += 1;
            continue;
        }
        // Display-only blocks: a body that is just `{{#include …}}` directive(s)
        // is inlined at site-build time from a fixture that lives outside the Book
        // (site/src/diagnostics/*.bynk). The fixture's own compile is checked by
        // tests/doc_diagnostics.rs, so don't demand it stand alone here.
        let nonempty: Vec<&str> = b
            .body
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        if !nonempty.is_empty() && nonempty.iter().all(|l| l.starts_with("{{#include")) {
            skip_include += 1;
            continue;
        }
        let expect_fail = b.info.contains("fail");
        let first = first_line(&b.body);

        let result: Result<(), String> = if first.starts_with("commons ") {
            bynkc::compile(&b.body, &b.file)
                .map(|_| ())
                .map_err(|errs| {
                    errs.iter()
                        .map(|e| format!("{}: {}", e.category, e.message))
                        .collect::<Vec<_>>()
                        .join("; ")
                })
        } else if first.starts_with("context ") {
            compile_context(&b.body, idx)
        } else {
            // A fragment, `test` block, or bare expression — cannot stand alone.
            if expect_fail {
                failures.push(format!(
                    "{loc}: marked `fail` but is not a standalone commons/context block"
                ));
            }
            skip_fragment += 1;
            continue;
        };

        match (expect_fail, result) {
            (false, Ok(())) => checked_ok += 1,
            (true, Err(_)) => checked_fail += 1,
            (false, Err(e)) => failures.push(format!("{loc}: expected to compile, but: {e}")),
            (true, Ok(())) => {
                failures.push(format!("{loc}: marked `fail` but compiled successfully"))
            }
        }
    }

    eprintln!(
        "doc examples: {checked_ok} compiled, {checked_fail} failed-as-expected, \
         {skip_fragment} fragments skipped, {skip_ignored} ignored, \
         {skip_include} include-only skipped ({} total)",
        blocks.len()
    );

    assert!(
        failures.is_empty(),
        "doc example compilation gate failed:\n  {}",
        failures.join("\n  ")
    );
}
