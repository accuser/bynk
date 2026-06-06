//! Generates the grammar reference page from `tree-sitter-karn/src/grammar.json`
//! and keeps it up to date.
//!
//! `docs/src/reference/grammar.md` is rendered from the compiled grammar, so it
//! cannot drift from the parser. Regenerate after a grammar change with:
//!     KARN_BLESS=1 cargo test -p karnc --test grammar_reference

use std::fs;
use std::path::PathBuf;

use serde_json::Value;

/// Render a grammar node to EBNF text plus a precedence level:
/// 0 = choice (`a | b`), 1 = sequence (`a b`), 2 = atom / postfix (`x`, `(…)*`).
fn render(node: &Value) -> (String, u8) {
    match node.get("type").and_then(Value::as_str).unwrap_or("") {
        "SYMBOL" => (node["name"].as_str().unwrap_or("?").to_string(), 2),
        "STRING" => (format!("\"{}\"", node["value"].as_str().unwrap_or("")), 2),
        "PATTERN" => (format!("/{}/", node["value"].as_str().unwrap_or("")), 2),
        "BLANK" => ("ε".to_string(), 2),
        // Wrappers that don't affect the surface grammar: render their content.
        "PREC" | "PREC_LEFT" | "PREC_RIGHT" | "PREC_DYNAMIC" | "TOKEN" | "IMMEDIATE_TOKEN"
        | "FIELD" | "ALIAS" => render(&node["content"]),
        "REPEAT" => (format!("{}*", wrap_atom(&node["content"])), 2),
        "REPEAT1" => (format!("{}+", wrap_atom(&node["content"])), 2),
        "SEQ" => {
            let parts: Vec<String> = members(node).iter().map(|m| wrap(m, 1)).collect();
            (parts.join(" "), 1)
        }
        "CHOICE" => {
            let all = members(node);
            let has_blank = all
                .iter()
                .any(|m| m.get("type").and_then(Value::as_str) == Some("BLANK"));
            let non_blank: Vec<&Value> = all
                .iter()
                .filter(|m| m.get("type").and_then(Value::as_str) != Some("BLANK"))
                .collect();
            if has_blank {
                // An optional: `X?`.
                if non_blank.len() == 1 {
                    (format!("{}?", wrap_atom(non_blank[0])), 2)
                } else {
                    let inner: Vec<String> = non_blank.iter().map(|m| render(m).0).collect();
                    (format!("({})?", inner.join(" | ")), 2)
                }
            } else {
                let inner: Vec<String> = non_blank.iter().map(|m| render(m).0).collect();
                (inner.join(" | "), 0)
            }
        }
        other => (format!("/* {other} */"), 2),
    }
}

fn members(node: &Value) -> Vec<Value> {
    node["members"].as_array().cloned().unwrap_or_default()
}

/// Wrap so the result can be a postfix operand (`*`, `+`, `?`): needs an atom.
fn wrap_atom(node: &Value) -> String {
    wrap(node, 2)
}

/// Wrap `node`'s rendering in parens if its level is below `min`.
fn wrap(node: &Value, min: u8) -> String {
    let (s, level) = render(node);
    if level < min { format!("({s})") } else { s }
}

fn render_extra(node: &Value) -> String {
    match node.get("type").and_then(Value::as_str).unwrap_or("") {
        "SYMBOL" => format!("`{}`", node["name"].as_str().unwrap_or("?")),
        "PATTERN" => format!("`/{}/`", node["value"].as_str().unwrap_or("")),
        "STRING" => format!("`\"{}\"`", node["value"].as_str().unwrap_or("")),
        _ => "?".to_string(),
    }
}

fn render_markdown(grammar_json: &str) -> String {
    let grammar: Value = serde_json::from_str(grammar_json).expect("grammar.json parses");

    let mut out = String::new();
    out.push_str("# Grammar\n\n");
    out.push_str(
        "<!-- GENERATED FILE — do not edit by hand.\n     \
         Source: tree-sitter-karn/src/grammar.json, via karnc/tests/grammar_reference.rs.\n     \
         Regenerate with: KARN_BLESS=1 cargo test -p karnc --test grammar_reference -->\n\n",
    );
    out.push_str(
        "The complete Karn grammar, generated from the `tree-sitter-karn` grammar.\n\n",
    );
    out.push_str("**Notation.** ");
    out.push_str(
        "`\"x\"` a literal token · `/x/` a regular expression · `( … )?` optional · \
         `( … )*` zero or more · `( … )+` one or more · `a | b` choice · `ε` empty. \
         Rule names beginning with `_` are internal helper rules (inlined into the \
         syntax tree). `doc_block` is an external token — a `--- … ---` documentation \
         block.\n\n",
    );

    out.push_str("```ebnf\n");
    if let Some(rules) = grammar.get("rules").and_then(Value::as_object) {
        for (name, body) in rules {
            let (rendered, _) = render(body);
            out.push_str(&format!("{name} ::= {rendered}\n"));
        }
    }
    out.push_str("```\n\n");

    out.push_str("## Tokens & trivia\n\n");
    if let Some(word) = grammar.get("word").and_then(Value::as_str) {
        out.push_str(&format!("- **Word token:** `{word}`\n"));
    }
    if let Some(extras) = grammar.get("extras").and_then(Value::as_array) {
        let rendered: Vec<String> = extras.iter().map(render_extra).collect();
        out.push_str(&format!(
            "- **Ignored between tokens:** {}\n",
            rendered.join(", ")
        ));
    }
    if let Some(externals) = grammar.get("externals").and_then(Value::as_array) {
        let rendered: Vec<String> = externals.iter().map(render_extra).collect();
        out.push_str(&format!("- **External tokens:** {}\n", rendered.join(", ")));
    }

    out
}

fn grammar_json() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../tree-sitter-karn/src/grammar.json");
    fs::read_to_string(path).expect("read grammar.json")
}

#[test]
fn generated_grammar_page_is_up_to_date() {
    let page = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../docs/src/reference/grammar.md");
    let rendered = render_markdown(&grammar_json());

    if std::env::var_os("KARN_BLESS").is_some() {
        fs::write(&page, &rendered).unwrap();
        return;
    }

    let current = fs::read_to_string(&page).unwrap_or_default();
    assert_eq!(
        current, rendered,
        "docs/src/reference/grammar.md is out of date with the grammar.\n\
         Regenerate with: KARN_BLESS=1 cargo test -p karnc --test grammar_reference"
    );
}
