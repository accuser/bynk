//! `mdbook-bynk-grammar` — an mdBook preprocessor that embeds a single grammar
//! production into a page by name.
//!
//! In any chapter, a line whose trimmed content is exactly `{{#grammar <rule>}}`
//! is replaced with an `ebnf` fenced block containing that production, rendered
//! from `tree-sitter-bynk/src/grammar.json` via the `bynk-grammar` crate. The
//! production therefore cannot drift from the parser or from the full grammar
//! appendix. An unknown rule fails the build loudly.
//!
//! We emit `ebnf` (not `bynk`) so the result composes with the `bynk-highlight`
//! preprocessor and the doc-example gate regardless of preprocessor ordering.
//!
//! Protocol (mdBook preprocessor):
//!   * `mdbook-bynk-grammar supports <renderer>` → exit 0 iff supported.
//!   * otherwise: stdin is `[context, book]` JSON; stdout is the modified book.

use std::io::Read;
use std::path::Path;
use std::process::exit;

use serde_json::Value;

/// The grammar path relative to the book root when none is configured.
const DEFAULT_GRAMMAR: &str = "../tree-sitter-bynk/src/grammar.json";

/// The grammar-semantics map path relative to the book root when none is
/// configured.
const DEFAULT_SEMANTICS: &str = "grammar-semantics.json";

/// The generated sources both directives read from.
struct Sources {
    /// Raw `grammar.json`, for `{{#grammar <rule>}}`.
    grammar: String,
    /// The `{ "<rule>": [ { code, summary }, … ] }` map, for
    /// `{{#grammar-semantics <rule>}}`.
    semantics: Value,
}

/// Expand a `{{#grammar <rule>}}` line into an `ebnf` fenced block holding that
/// production. Exits non-zero on an unknown rule so a typo fails the build.
fn expand_grammar(rule: &str, grammar_json: &str, out: &mut String) {
    match bynk_grammar::render_production(grammar_json, rule) {
        Ok(line) => {
            out.push_str("```ebnf\n");
            out.push_str(&line);
            out.push('\n');
            out.push_str("```\n");
        }
        Err(e) => {
            eprintln!("mdbook-bynk-grammar: {e}");
            exit(1);
        }
    }
}

/// Expand a `{{#grammar-semantics <rule>}}` line into a bullet list of the
/// diagnostics that constrain `rule`, with a link to the full index. A rule with
/// no diagnostics is legitimate, so emit a neutral line rather than failing.
fn expand_semantics(rule: &str, semantics: &Value, out: &mut String) {
    let diags = semantics.get(rule).and_then(Value::as_array);
    match diags {
        Some(arr) if !arr.is_empty() => {
            for diag in arr {
                let code = diag.get("code").and_then(Value::as_str).unwrap_or("");
                let summary = diag.get("summary").and_then(Value::as_str).unwrap_or("");
                out.push_str(&format!("- `{code}` — {summary}\n"));
            }
            out.push_str("\nSee the [diagnostic index](diagnostics.md) for all codes.\n");
        }
        _ => {
            out.push_str("_No diagnostics constrain this construct directly._\n");
        }
    }
}

/// Rewrite every `{{#grammar <rule>}}` and `{{#grammar-semantics <rule>}}` line
/// in a chapter's Markdown. Other lines are left untouched.
fn process_markdown(content: &str, sources: &Sources) -> String {
    let mut out = String::new();
    for line in content.lines() {
        let trimmed = line.trim();
        // Allow a directive to be shown literally (e.g. in the contributor docs)
        // by escaping it: `\{{#grammar …}}` renders as `{{#grammar …}}` with the
        // backslash consumed, mirroring mdBook's own escape convention.
        if trimmed.starts_with("\\{{#grammar") {
            out.push_str(&line.replacen('\\', "", 1));
            out.push('\n');
            continue;
        }
        if let Some(rule) = trimmed
            .strip_prefix("{{#grammar-semantics ")
            .and_then(|rest| rest.strip_suffix("}}"))
        {
            expand_semantics(rule.trim(), &sources.semantics, &mut out);
        } else if let Some(rule) = trimmed
            .strip_prefix("{{#grammar ")
            .and_then(|rest| rest.strip_suffix("}}"))
        {
            expand_grammar(rule.trim(), &sources.grammar, &mut out);
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// Recursively transform every chapter's content in the book's section list.
fn process_items(items: &mut Vec<Value>, sources: &Sources) {
    for item in items {
        if let Some(chapter) = item.get_mut("Chapter").and_then(Value::as_object_mut) {
            if let Some(content) = chapter.get("content").and_then(Value::as_str) {
                let rewritten = process_markdown(content, sources);
                chapter.insert("content".to_string(), Value::String(rewritten));
            }
            if let Some(sub) = chapter.get_mut("sub_items").and_then(Value::as_array_mut) {
                process_items(sub, sources);
            }
        }
    }
}

/// Read the grammar JSON named by the preprocessor context: the configured
/// `grammar` path (or the default), resolved relative to the book `root`.
fn load_grammar(context: &Value) -> String {
    let root = context.get("root").and_then(Value::as_str).unwrap_or(".");
    let rel = context
        .get("config")
        .and_then(|c| c.get("preprocessor"))
        .and_then(|p| p.get("bynk-grammar"))
        .and_then(|g| g.get("grammar"))
        .and_then(Value::as_str)
        .unwrap_or(DEFAULT_GRAMMAR);
    let path = Path::new(root).join(rel);
    match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "mdbook-bynk-grammar: cannot read grammar at {}: {e}",
                path.display()
            );
            exit(1);
        }
    }
}

/// Read and parse the grammar-semantics map named by the preprocessor context:
/// the configured `semantics` path (or the default), resolved relative to the
/// book `root`.
fn load_semantics(context: &Value) -> Value {
    let root = context.get("root").and_then(Value::as_str).unwrap_or(".");
    let rel = context
        .get("config")
        .and_then(|c| c.get("preprocessor"))
        .and_then(|p| p.get("bynk-grammar"))
        .and_then(|g| g.get("semantics"))
        .and_then(Value::as_str)
        .unwrap_or(DEFAULT_SEMANTICS);
    let path = Path::new(root).join(rel);
    let text = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "mdbook-bynk-grammar: cannot read grammar semantics at {}: {e}",
                path.display()
            );
            exit(1);
        }
    };
    match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("mdbook-bynk-grammar: invalid grammar semantics JSON: {e}");
            exit(1);
        }
    }
}

/// Transform a parsed `[context, book]` value in place.
fn process_book(parsed: &mut Value) {
    let sources = Sources {
        grammar: load_grammar(&parsed[0]),
        semantics: load_semantics(&parsed[0]),
    };
    let book = &mut parsed[1];
    if let Some(sections) = book.get_mut("sections").and_then(Value::as_array_mut) {
        process_items(sections, &sources);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // `supports <renderer>`: we only rewrite HTML output.
    if args.len() >= 3 && args[1] == "supports" {
        exit(if args[2] == "html" { 0 } else { 1 });
    }

    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        eprintln!("mdbook-bynk-grammar: failed to read stdin");
        exit(1);
    }
    let mut parsed: Value = match serde_json::from_str(&input) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("mdbook-bynk-grammar: invalid preprocessor input: {e}");
            exit(1);
        }
    };

    process_book(&mut parsed);

    // Input is `[context, book]`; we emit the transformed book.
    match serde_json::to_string(&parsed[1]) {
        Ok(s) => println!("{s}"),
        Err(e) => {
            eprintln!("mdbook-bynk-grammar: failed to serialise book: {e}");
            exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A `[context, book]` whose single chapter holds `content`, pointing the
    /// directives at the repository's real generated sources.
    fn book_with(content: &str) -> Value {
        let root = env!("CARGO_MANIFEST_DIR");
        json!([
            {
                "root": root,
                "config": {
                    "preprocessor": {
                        "bynk-grammar": {
                            "grammar": "../tree-sitter-bynk/src/grammar.json",
                            "semantics": "../docs/grammar-semantics.json"
                        }
                    }
                }
            },
            {
                "sections": [
                    { "Chapter": { "content": content, "sub_items": [] } }
                ]
            }
        ])
    }

    fn rendered_chapter(parsed: &Value) -> String {
        parsed[1]["sections"][0]["Chapter"]["content"]
            .as_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn directive_renders_production_in_ebnf_fence() {
        let mut parsed = book_with("intro\n{{#grammar match_arm}}\noutro\n");
        process_book(&mut parsed);

        let content = rendered_chapter(&parsed);
        assert!(
            content.contains("```ebnf\n"),
            "missing ebnf fence:\n{content}"
        );
        assert!(
            content.contains("match_arm ::= pattern \"=>\" expression \",\"?"),
            "missing rendered production:\n{content}"
        );
        // Surrounding lines are preserved.
        assert!(content.contains("intro\n"));
        assert!(content.contains("outro\n"));
    }

    #[test]
    fn semantics_directive_lists_diagnostics() {
        let mut parsed = book_with("{{#grammar-semantics http_handler}}\n");
        process_book(&mut parsed);

        let content = rendered_chapter(&parsed);
        assert!(
            content.contains("- `bynk.http.path_param_not_stringy` — "),
            "missing http diagnostic bullet:\n{content}"
        );
        assert!(
            content.contains("- `bynk.http.body_on_get_or_delete` — "),
            "missing http diagnostic bullet:\n{content}"
        );
        assert!(
            content.contains("[diagnostic index](diagnostics.md)"),
            "missing index link:\n{content}"
        );
    }

    #[test]
    fn escaped_directive_renders_literally() {
        let mut parsed = book_with("\\{{#grammar http_handler}}\n");
        process_book(&mut parsed);

        let content = rendered_chapter(&parsed);
        assert!(
            content.contains("{{#grammar http_handler}}"),
            "expected literal directive:\n{content}"
        );
        // It must NOT have been expanded into a production.
        assert!(
            !content.contains("```ebnf"),
            "escaped directive was expanded:\n{content}"
        );
    }

    #[test]
    fn semantics_directive_neutral_when_unconstrained() {
        // `paren_expr` is a real rule with no diagnostics mapped to it.
        let mut parsed = book_with("{{#grammar-semantics paren_expr}}\n");
        process_book(&mut parsed);

        let content = rendered_chapter(&parsed);
        assert!(
            content.contains("_No diagnostics constrain this construct directly._"),
            "expected neutral line:\n{content}"
        );
    }
}
