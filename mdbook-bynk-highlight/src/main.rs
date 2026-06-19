//! `mdbook-bynk-highlight` — an mdBook preprocessor that syntax-highlights
//! fenced ```bynk code blocks using the `tree-sitter-bynk` grammar.
//!
//! The grammar (and its highlight queries) are the single source of truth, so
//! highlighting stays correct as the language evolves. Highlighted blocks are
//! emitted as `<pre class="bynk"><code>…</code></pre>` with `hl-*` span classes;
//! the colours live in `docs/theme/bynk-highlight.css`.
//!
//! Protocol (mdBook preprocessor):
//!   * `mdbook-bynk-highlight supports <renderer>` → exit 0 iff supported.
//!   * otherwise: stdin is `[context, book]` JSON; stdout is the modified book.

use std::io::Read;
use std::process::exit;
use std::sync::OnceLock;

use serde_json::Value;
use tree_sitter_highlight::{HighlightConfiguration, Highlighter, HtmlRenderer};

/// Highlight names recognised in `highlights.scm`. The `Highlight(usize)` we get
/// back indexes into this list, so the order here defines the class mapping.
const HIGHLIGHT_NAMES: &[&str] = &[
    "attribute",
    "comment",
    "comment.documentation",
    "constant",
    "constant.builtin",
    "error",
    "field",
    "function",
    "function.builtin",
    "function.method",
    "keyword",
    "keyword.declaration",
    "keyword.import",
    "keyword.modifier",
    "keyword.operator",
    "module",
    "number",
    "operator",
    "punctuation.bracket",
    "punctuation.delimiter",
    "punctuation.special",
    "string",
    "type",
    "type.builtin",
    "variable",
    "variable.builtin",
    "variable.parameter",
];

unsafe extern "C" {
    fn tree_sitter_bynk() -> *const ();
}

fn highlight_config() -> &'static HighlightConfiguration {
    static CONFIG: OnceLock<HighlightConfiguration> = OnceLock::new();
    CONFIG.get_or_init(|| {
        let language = unsafe { tree_sitter::Language::from_raw(tree_sitter_bynk().cast()) };
        let mut config = HighlightConfiguration::new(
            language,
            "bynk",
            include_str!("../../tree-sitter-bynk/queries/highlights.scm"),
            include_str!("../../tree-sitter-bynk/queries/injections.scm"),
            "",
        )
        .expect("bynk highlight queries should compile");
        config.configure(HIGHLIGHT_NAMES);
        config
    })
}

/// Per-highlight `class="hl-…"` attribute bytes, indexed like `HIGHLIGHT_NAMES`.
fn class_attrs() -> &'static [String] {
    static ATTRS: OnceLock<Vec<String>> = OnceLock::new();
    ATTRS.get_or_init(|| {
        HIGHLIGHT_NAMES
            .iter()
            .map(|name| format!("class=\"hl-{}\"", name.replace('.', "-")))
            .collect()
    })
}

/// Highlight one block of Bynk source into inner `<code>` HTML.
fn highlight_bynk(code: &str) -> Option<String> {
    let config = highlight_config();
    let attrs = class_attrs();
    let mut highlighter = Highlighter::new();
    let events = highlighter
        .highlight(config, code.as_bytes(), None, |_| None)
        .ok()?;
    let mut renderer = HtmlRenderer::new();
    renderer
        .render(events, code.as_bytes(), &|h, out: &mut Vec<u8>| {
            out.extend_from_slice(attrs[h.0].as_bytes());
        })
        .ok()?;
    String::from_utf8(renderer.html).ok()
}

/// Rewrite every fenced ```bynk block in a chapter's Markdown to highlighted
/// HTML. Other fenced blocks (```typescript, ```text, …) are left untouched.
fn process_markdown(content: &str) -> String {
    let mut out = String::new();
    let mut lines = content.lines().peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        // Match a ```bynk fence (with or without a `,annotation` suffix), but
        // not ```bynkx or other languages.
        let is_bynk_fence = trimmed.strip_prefix("```bynk").is_some_and(|rest| {
            rest.is_empty() || rest.starts_with(',') || rest.starts_with(char::is_whitespace)
        });
        if is_bynk_fence {
            let mut code = String::new();
            for body in lines.by_ref() {
                if body.trim() == "```" {
                    break;
                }
                code.push_str(body);
                code.push('\n');
            }
            match highlight_bynk(&code) {
                Some(html) => {
                    out.push_str("\n<pre class=\"bynk\"><code>");
                    out.push_str(&html);
                    out.push_str("</code></pre>\n\n");
                }
                None => {
                    // Fall back to a plain code block if highlighting failed.
                    out.push_str("```bynk\n");
                    out.push_str(&code);
                    out.push_str("```\n");
                }
            }
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// Recursively transform every chapter's content in the book's section list.
fn process_items(items: &mut Vec<Value>) {
    for item in items {
        if let Some(chapter) = item.get_mut("Chapter").and_then(Value::as_object_mut) {
            if let Some(content) = chapter.get("content").and_then(Value::as_str) {
                let rewritten = process_markdown(content);
                chapter.insert("content".to_string(), Value::String(rewritten));
            }
            if let Some(sub) = chapter.get_mut("sub_items").and_then(Value::as_array_mut) {
                process_items(sub);
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // `supports <renderer>`: we only highlight HTML output.
    if args.len() >= 3 && args[1] == "supports" {
        exit(if args[2] == "html" { 0 } else { 1 });
    }

    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        eprintln!("mdbook-bynk-highlight: failed to read stdin");
        exit(1);
    }
    let mut parsed: Value = match serde_json::from_str(&input) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("mdbook-bynk-highlight: invalid preprocessor input: {e}");
            exit(1);
        }
    };

    // Input is `[context, book]`; we transform and emit the book.
    let book = &mut parsed[1];
    if let Some(sections) = book.get_mut("sections").and_then(Value::as_array_mut) {
        process_items(sections);
    }

    match serde_json::to_string(book) {
        Ok(s) => println!("{s}"),
        Err(e) => {
            eprintln!("mdbook-bynk-highlight: failed to serialise book: {e}");
            exit(1);
        }
    }
}
