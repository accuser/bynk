//! `mdbook-bynk-visuals` — an mdBook preprocessor for diagrams and callouts.
//!
//! Two transforms, both pure text-to-Markdown/HTML so they compose with the
//! other in-house preprocessors and stay deterministic and offline (no CDN, no
//! external plugin pinned against the 0.4.x protocol):
//!
//!   * **Diagrams.** A ` ```mermaid ` fenced block becomes
//!     `<pre class="mermaid">…</pre>`, which `theme/mermaid.min.js` (vendored)
//!     renders client-side via `theme/mermaid-init.js`. `<pre>` is a CommonMark
//!     type-1 HTML block, so the diagram body passes through verbatim.
//!   * **Callouts.** A GitHub-style alert blockquote whose first line is
//!     `[!NOTE]` / `[!TIP]` / `[!WARNING]` / `[!DANGER]` becomes a
//!     `<div class="callout callout-<kind>">…</div>`, styled by
//!     `theme/bynk-callouts.css`. Blank lines around the body keep its inner
//!     Markdown rendering (a CommonMark type-6 HTML block ends at a blank line).
//!     An unrecognised `[!KIND]` is left as an ordinary blockquote.
//!
//! Directives inside other fenced code blocks are left untouched, so docs can
//! show the syntax literally.
//!
//! Protocol (mdBook preprocessor):
//!   * `mdbook-bynk-visuals supports <renderer>` → exit 0 iff supported.
//!   * otherwise: stdin is `[context, book]` JSON; stdout is the modified book.

use std::io::Read;
use std::process::exit;

use serde_json::Value;

/// The four callout kinds, mapped from `[!KIND]` to `(css-class, Title)`.
fn callout_kind(line: &str) -> Option<(&'static str, &'static str)> {
    let rest = line.strip_prefix('>')?.trim();
    let inner = rest.strip_prefix("[!")?.strip_suffix(']')?;
    match inner.to_ascii_uppercase().as_str() {
        "NOTE" => Some(("note", "Note")),
        "TIP" => Some(("tip", "Tip")),
        "WARNING" => Some(("warning", "Warning")),
        "DANGER" => Some(("danger", "Danger")),
        _ => None,
    }
}

/// Strip one level of blockquote marker (`>` and an optional following space).
fn dequote(line: &str) -> String {
    let t = line.trim_start();
    let after = t.strip_prefix('>').unwrap_or(t);
    after.strip_prefix(' ').unwrap_or(after).to_string()
}

fn render_callout(class: &str, title: &str, body: &[String]) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "\n<div class=\"callout callout-{class}\">\n<p class=\"callout-title\">{title}</p>\n\n"
    ));
    for line in body {
        s.push_str(line);
        s.push('\n');
    }
    s.push_str("\n</div>\n\n");
    s
}

/// Rewrite ` ```mermaid ` blocks and `[!KIND]` callout blockquotes in a
/// chapter's Markdown. Other content — including other fenced code blocks — is
/// left untouched.
fn process_markdown(content: &str) -> String {
    let mut out = String::new();
    let mut lines = content.lines().peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();

        // Fenced code blocks.
        if let Some(info) = trimmed.strip_prefix("```") {
            if info.trim() == "mermaid" {
                let mut body = String::new();
                for l in lines.by_ref() {
                    if l.trim() == "```" {
                        break;
                    }
                    body.push_str(l);
                    body.push('\n');
                }
                out.push_str("\n<pre class=\"mermaid\">\n");
                out.push_str(&body);
                out.push_str("</pre>\n\n");
                continue;
            }
            // Any other fence: emit verbatim through its close, so callout or
            // mermaid syntax shown in an example is never transformed.
            out.push_str(line);
            out.push('\n');
            for l in lines.by_ref() {
                out.push_str(l);
                out.push('\n');
                if l.trim() == "```" {
                    break;
                }
            }
            continue;
        }

        // Callout blockquote.
        if let Some((class, title)) = callout_kind(trimmed) {
            let mut body: Vec<String> = Vec::new();
            while let Some(peek) = lines.peek() {
                if peek.trim_start().starts_with('>') {
                    body.push(dequote(lines.next().unwrap()));
                } else {
                    break;
                }
            }
            out.push_str(&render_callout(class, title, &body));
            continue;
        }

        out.push_str(line);
        out.push('\n');
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
    // `supports <renderer>`: we only transform HTML output.
    if args.len() >= 3 && args[1] == "supports" {
        exit(if args[2] == "html" { 0 } else { 1 });
    }

    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        eprintln!("mdbook-bynk-visuals: failed to read stdin");
        exit(1);
    }
    let mut parsed: Value = match serde_json::from_str(&input) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("mdbook-bynk-visuals: invalid preprocessor input: {e}");
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
            eprintln!("mdbook-bynk-visuals: failed to serialise book: {e}");
            exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mermaid_fence_becomes_pre() {
        let md = "intro\n\n```mermaid\nflowchart LR\n  A --> B\n```\n\nafter\n";
        let out = process_markdown(md);
        assert!(out.contains("<pre class=\"mermaid\">"), "{out}");
        assert!(out.contains("flowchart LR"), "{out}");
        assert!(out.contains("A --> B"), "{out}");
        assert!(!out.contains("```mermaid"), "{out}");
        assert!(out.contains("after"), "{out}");
    }

    #[test]
    fn callout_becomes_div_with_markdown_body() {
        let md = "> [!DANGER]\n> The `/_bynk/` prefix is **reserved**.\n";
        let out = process_markdown(md);
        assert!(
            out.contains("<div class=\"callout callout-danger\">"),
            "{out}"
        );
        assert!(
            out.contains("<p class=\"callout-title\">Danger</p>"),
            "{out}"
        );
        // Body preserved as Markdown (blank lines around it let it render).
        assert!(
            out.contains("The `/_bynk/` prefix is **reserved**."),
            "{out}"
        );
        assert!(out.contains("</div>"), "{out}");
    }

    #[test]
    fn each_kind_maps_to_its_class() {
        for (marker, class, title) in [
            ("note", "note", "Note"),
            ("tip", "tip", "Tip"),
            ("warning", "warning", "Warning"),
            ("danger", "danger", "Danger"),
        ] {
            let md = format!("> [!{}]\n> body\n", marker.to_uppercase());
            let out = process_markdown(&md);
            assert!(out.contains(&format!("callout-{class}")), "{out}");
            assert!(out.contains(&format!(">{title}</p>")), "{out}");
        }
    }

    #[test]
    fn ordinary_blockquote_is_untouched() {
        let md = "> just a quote\n> second line\n";
        let out = process_markdown(md);
        assert!(out.contains("> just a quote"), "{out}");
        assert!(!out.contains("callout"), "{out}");
    }

    #[test]
    fn directives_inside_code_fences_are_not_transformed() {
        let md = "```text\n> [!NOTE]\n> shown literally\n```\n";
        let out = process_markdown(md);
        assert!(out.contains("> [!NOTE]"), "{out}");
        assert!(!out.contains("callout"), "{out}");
    }
}
