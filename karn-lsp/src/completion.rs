//! Completion for the cursor, keyed off the line up to it.
//!
//! v0.17 recognised three adapter/capability contexts:
//!
//! - after `consumes ` — consumable units (contexts, adapters, and the `karn`
//!   surface);
//! - inside `consumes U { … }` — the capabilities `U` exports;
//! - after `given …` — the capabilities in scope (local, flattened via a braced
//!   `consumes`, and qualified `U.Cap` for whole-unit `consumes`).
//!
//! v0.30 slice 1 adds **positional** completion — contexts that need neither
//! receiver typing nor scope tracking:
//!
//! - **type position** (after `:`, in `-> T`, inside a `[ … ]` type-argument
//!   list) — built-in types, the `karn`-surface transparent types, and project
//!   type declarations;
//! - **keyword position** (a bare word at a declaration/statement start) — the
//!   reserved keywords (with their registry docs) and declaration snippets.
//!
//! Context detection is lexical (it must work mid-edit, when the buffer rarely
//! parses); candidates are semantic. Unit/type/capability enumeration parses
//! the project's `.karn` files (and the embedded `karn` surface) with recovery,
//! so it works even while the file the cursor sits in is mid-edit. Built-ins
//! and keywords come from the static `karnc` registries (`keywords`/
//! `builtin_names`/`firstparty`), never the index — first-party symbols aren't
//! indexed (the v0.28 finding). `.`-member and locals/params-in-scope need
//! receiver typing + a scope-at-offset query and are deferred to slice 2.

use std::collections::BTreeSet;
use std::path::Path;

use karnc::ast::{CommonsItem, ExportKind, SourceUnit};
use karnc::firstparty::{CLOUDFLARE_ADAPTER_SRC, KARN_ADAPTER_SRC};
use karnc::{keywords, lexer, parser};

use crate::symbols::walk_karn_files;

/// What a candidate refers to — maps to an LSP `CompletionItemKind`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Unit,
    Capability,
    Type,
    Keyword,
    Snippet,
}

pub struct Completion {
    pub label: String,
    pub kind: CompletionKind,
    pub detail: Option<String>,
    /// LSP snippet text (with `${n:…}`/`$0` tab stops) for `Snippet` items;
    /// `None` means insert the label verbatim.
    pub insert_text: Option<String>,
}

impl Completion {
    fn item(label: impl Into<String>, kind: CompletionKind, detail: Option<String>) -> Self {
        Completion {
            label: label.into(),
            kind,
            detail,
            insert_text: None,
        }
    }

    fn snippet(label: &str, body: &str) -> Self {
        Completion {
            label: label.to_string(),
            kind: CompletionKind::Snippet,
            detail: Some(format!("{label} scaffold")),
            insert_text: Some(body.to_string()),
        }
    }
}

/// Produce completions for the cursor, given the text of the line up to the
/// cursor, the current document text, and the project source root (if any).
pub fn complete(line_prefix: &str, doc_text: &str, src_root: Option<&Path>) -> Vec<Completion> {
    // 1. Inside `consumes U { … <cursor>` — the capabilities U exports.
    if let Some(unit) = consumes_brace_unit(line_prefix) {
        return capabilities_of_unit(&unit, doc_text, src_root)
            .into_iter()
            .map(|c| {
                Completion::item(
                    c,
                    CompletionKind::Capability,
                    Some(format!("capability exported by `{unit}`")),
                )
            })
            .collect();
    }
    // 2. After `consumes <prefix>` — consumable unit names.
    if is_consumes_target(line_prefix) {
        return consumable_units(doc_text, src_root);
    }
    // 3. After `given …` — in-scope capabilities.
    if is_given_position(line_prefix) {
        return in_scope_capabilities(doc_text, src_root);
    }
    // 4. Type position (`: T`, `-> T`, `[ … ]` type args) — built-ins, the
    //    `karn`-surface transparent types, and project type declarations.
    if is_type_position(line_prefix) {
        return type_candidates(doc_text, src_root);
    }
    // 5. Keyword position (a bare word at a declaration/statement start) — the
    //    reserved keywords plus declaration snippets.
    if is_keyword_position(line_prefix) {
        return keyword_and_snippet_candidates();
    }
    Vec::new()
}

// -- Cursor-context detection (line-prefix scanning) --

/// `consumes U { … ` with the brace still open at the cursor → `Some(U)`.
fn consumes_brace_unit(line: &str) -> Option<String> {
    let idx = line.rfind("consumes")?;
    let after = &line[idx + "consumes".len()..];
    let open = after.find('{')?;
    // The brace must still be open up to the cursor (no closing brace after it).
    if after[open + 1..].contains('}') {
        return None;
    }
    let unit = after[..open].trim();
    if unit.is_empty() || !is_qualified_name(unit) {
        return None;
    }
    Some(unit.to_string())
}

/// `consumes <partial>` with no brace or `as` yet → completing the target name.
fn is_consumes_target(line: &str) -> bool {
    let Some(idx) = line.rfind("consumes") else {
        return false;
    };
    // `consumes` must be a standalone keyword (preceded by start/whitespace).
    if !line[..idx]
        .chars()
        .last()
        .map(|c| c.is_whitespace())
        .unwrap_or(true)
    {
        return false;
    }
    let after = &line[idx + "consumes".len()..];
    // Need at least one separating space, and no `{`, `}`, or `as` yet.
    after.starts_with(char::is_whitespace)
        && !after.contains('{')
        && !after.contains('}')
        && !after.split_whitespace().any(|w| w == "as")
}

/// The cursor is inside a `given` list (after `given`, before the `{` body).
fn is_given_position(line: &str) -> bool {
    let Some(idx) = line.rfind("given") else {
        return false;
    };
    if !line[..idx]
        .chars()
        .last()
        .map(|c| c.is_whitespace())
        .unwrap_or(true)
    {
        return false;
    }
    let after = &line[idx + "given".len()..];
    if !after.starts_with(char::is_whitespace) {
        return false;
    }
    // Still in the given list while only capability names, dots, commas and
    // whitespace follow — a `{` opens the handler body.
    after
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | ',' | ' ' | '\t'))
}

fn is_qualified_name(s: &str) -> bool {
    !s.is_empty()
        && s.split('.').all(|seg| {
            !seg.is_empty()
                && seg.chars().all(|c| c.is_alphanumeric() || c == '_')
                && !seg.chars().next().unwrap().is_ascii_digit()
        })
}

/// The cursor sits in a type position: a return type (`-> T`), a type
/// annotation/field type (`: T`), or inside a `[ … ]` type-argument list. The
/// partial type name being typed is stripped before inspecting the preceding
/// token, so `: Optio` and `-> Eff` both qualify.
///
/// Conservative by construction: a list literal `[1, 2` is excluded (its `[` is
/// not preceded by a type constructor). The one accepted false positive is a
/// record *construction* value (`Order { id: <cursor>`), lexically identical to
/// a record field-type declaration — offering type names there is mild noise.
fn is_type_position(line: &str) -> bool {
    let head = line
        .trim_end_matches(|c: char| c.is_alphanumeric() || c == '_')
        .trim_end();
    head.ends_with("->") || (head.ends_with(':') && !head.ends_with("::")) || in_type_arg_list(head)
}

/// `head` ends inside an unclosed `[ … ` whose opening bracket immediately
/// follows an identifier (a type constructor, e.g. `Option[`, `Result[Int, `) —
/// as opposed to a bare list-literal `[`.
fn in_type_arg_list(head: &str) -> bool {
    let chars: Vec<char> = head.chars().collect();
    let mut depth = 0i32;
    let mut opener_after_ident = false;
    for (i, &c) in chars.iter().enumerate() {
        match c {
            '[' => {
                depth += 1;
                if depth == 1 {
                    opener_after_ident =
                        i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_');
                }
            }
            ']' => depth -= 1,
            _ => {}
        }
    }
    depth > 0 && opener_after_ident
}

/// A bare word at a declaration/statement start: the line up to the cursor is
/// only leading whitespace plus an optional partial identifier (no operators,
/// colons, or brackets). Fires on an empty line too. Disjoint from
/// [`is_type_position`], whose triggers (`:`/`->`/`[`) make this false.
fn is_keyword_position(line: &str) -> bool {
    line.trim().chars().all(|c| c.is_alphanumeric() || c == '_')
}

// -- Positional candidate sources (static registries + project parse) --

/// Built-in type names not declared in any parseable source. Base and generic
/// types from the language core; collection types from `builtin_names`. Docs
/// are drawn from the `keywords` registry where present (one source of truth).
const BUILTIN_TYPES: &[&str] = &[
    karnc::builtin_names::types::INT,
    "Bool",
    karnc::builtin_names::types::FLOAT,
    "String",
    "Option",
    "Result",
    "Effect",
    karnc::builtin_names::types::LIST,
    karnc::builtin_names::types::MAP,
];

/// Declaration snippets (`CompletionItemKind::SNIPPET`), as LSP snippet bodies.
const SNIPPETS: &[(&str, &str)] = &[
    ("context", "context ${1:name} {\n\t$0\n}"),
    (
        "adapter",
        "adapter ${1:name} {\n\tbinding \"${2:./module}\"\n\t$0\n}",
    ),
    (
        "capability",
        "capability ${1:Name} {\n\tfn ${2:op}() -> Effect[${3:Unit}]\n}",
    ),
    (
        "service",
        "service ${1:name} {\n\ton call(${2}) -> Effect[${3:Unit}] {\n\t\t$0\n\t}\n}",
    ),
    ("on call", "on call(${1}) -> Effect[${2:Unit}] {\n\t$0\n}"),
    ("test", "test \"${1:description}\" {\n\t$0\n}"),
];

/// The one-line doc for a name in the `keywords` registry, if present.
fn keyword_doc(word: &str) -> Option<&'static str> {
    keywords::KEYWORDS
        .iter()
        .find(|k| k.word == word)
        .map(|k| k.meaning)
}

/// Type-position candidates: built-in types (with registry docs), then every
/// `type` declaration found in the project sources and the embedded `karn`
/// surface (so the transparent surface types `Uuid`/`Method`/… come for free).
fn type_candidates(doc_text: &str, src_root: Option<&Path>) -> Vec<Completion> {
    let mut out: Vec<Completion> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for &name in BUILTIN_TYPES {
        if seen.insert(name.to_string()) {
            let detail = keyword_doc(name)
                .map(str::to_string)
                .or_else(|| match name {
                    "List" => Some("The built-in list type, `List[T]`.".to_string()),
                    "Map" => Some("The built-in map type, `Map[K, V]`.".to_string()),
                    _ => Some("built-in type".to_string()),
                });
            out.push(Completion::item(name, CompletionKind::Type, detail));
        }
    }
    for_each_unit(doc_text, src_root, |unit| {
        let items = match unit {
            SourceUnit::Commons(c) => &c.items,
            SourceUnit::Context(c) => &c.items,
            SourceUnit::Adapter(a) => &a.items,
            _ => return,
        };
        for item in items {
            if let CommonsItem::Type(t) = item
                && seen.insert(t.name.name.clone())
            {
                out.push(Completion::item(
                    t.name.name.clone(),
                    CompletionKind::Type,
                    Some("type".to_string()),
                ));
            }
        }
    });
    out
}

/// Keyword-position candidates: the lowercase-initial reserved keywords (the
/// declaration/statement words — uppercase type/value names like `Int`/`Some`
/// belong to type/expression position) with their registry docs, plus the
/// declaration snippets.
fn keyword_and_snippet_candidates() -> Vec<Completion> {
    let mut out: Vec<Completion> = keywords::KEYWORDS
        .iter()
        .filter(|k| k.word.chars().next().is_some_and(char::is_lowercase))
        .map(|k| Completion::item(k.word, CompletionKind::Keyword, Some(k.meaning.to_string())))
        .collect();
    for &(label, body) in SNIPPETS {
        out.push(Completion::snippet(label, body));
    }
    out
}

// -- Enumeration (parse project sources + the embedded `karn` surface) --

/// Parse every project unit, plus the embedded first-party adapters (the
/// `karn` surface and the `karn.cloudflare` platform adapter), and call `f`
/// for each. Recovery parsing tolerates the in-progress edit at the cursor.
fn for_each_unit(doc_text: &str, src_root: Option<&Path>, mut f: impl FnMut(&SourceUnit)) {
    let mut sources: Vec<String> = vec![
        KARN_ADAPTER_SRC.to_string(),
        CLOUDFLARE_ADAPTER_SRC.to_string(),
        doc_text.to_string(),
    ];
    if let Some(root) = src_root {
        for path in walk_karn_files(root) {
            if let Ok(s) = std::fs::read_to_string(&path) {
                sources.push(s);
            }
        }
    }
    for src in &sources {
        let Ok(tokens) = lexer::tokenize(src) else {
            continue;
        };
        let (unit, _errs) = parser::parse_unit_with_recovery(&tokens, src);
        if let Some(unit) = unit {
            f(&unit);
        }
    }
}

/// Consumable unit names: contexts and adapters (plus `karn`), deduplicated.
fn consumable_units(doc_text: &str, src_root: Option<&Path>) -> Vec<Completion> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut out: Vec<Completion> = Vec::new();
    for_each_unit(doc_text, src_root, |unit| {
        let (name, kind) = match unit {
            SourceUnit::Context(c) => (c.name.joined(), "context"),
            SourceUnit::Adapter(a) => (a.name.joined(), "adapter"),
            _ => return,
        };
        if seen.insert(name.clone()) {
            out.push(Completion::item(
                name,
                CompletionKind::Unit,
                Some(kind.to_string()),
            ));
        }
    });
    out
}

/// The capability names a unit `exports capability`.
fn capabilities_of_unit(unit: &str, doc_text: &str, src_root: Option<&Path>) -> Vec<String> {
    let mut out: BTreeSet<String> = BTreeSet::new();
    for_each_unit(doc_text, src_root, |u| {
        let (name, exports) = match u {
            SourceUnit::Context(c) => (c.name.joined(), &c.exports),
            SourceUnit::Adapter(a) => (a.name.joined(), &a.exports),
            _ => return,
        };
        if name != unit {
            return;
        }
        for clause in exports {
            if clause.kind == ExportKind::Capability {
                for n in &clause.names {
                    out.insert(n.name.clone());
                }
            }
        }
    });
    out.into_iter().collect()
}

/// Capabilities in scope for a `given` clause in the current document: locally
/// declared capabilities, bare names flattened by a braced `consumes`, and
/// `U.Cap` for each whole-unit `consumes U`.
fn in_scope_capabilities(doc_text: &str, src_root: Option<&Path>) -> Vec<Completion> {
    let mut labels: BTreeSet<String> = BTreeSet::new();
    let Ok(tokens) = lexer::tokenize(doc_text) else {
        return Vec::new();
    };
    let (Some(unit), _errs) = parser::parse_unit_with_recovery(&tokens, doc_text) else {
        return Vec::new();
    };
    let (items, consumes) = match &unit {
        SourceUnit::Context(c) => (&c.items, &c.consumes),
        SourceUnit::Adapter(a) => (&a.items, &EMPTY_CONSUMES),
        _ => return Vec::new(),
    };
    // Locally declared capabilities.
    for item in items {
        if let karnc::ast::CommonsItem::Capability(c) = item {
            labels.insert(c.name.name.clone());
        }
    }
    // Consumed capabilities: flattened bare names, or qualified `U.Cap`.
    for c in consumes {
        let unit_name = c.target.joined();
        match &c.selected {
            Some(names) => {
                for n in names {
                    labels.insert(n.name.clone());
                }
            }
            None => {
                let prefix = c
                    .alias
                    .as_ref()
                    .map(|a| a.name.clone())
                    .unwrap_or_else(|| unit_name.clone());
                for cap in capabilities_of_unit(&unit_name, doc_text, src_root) {
                    labels.insert(format!("{prefix}.{cap}"));
                }
            }
        }
    }
    labels
        .into_iter()
        .map(|label| {
            Completion::item(
                label,
                CompletionKind::Capability,
                Some("capability in scope".to_string()),
            )
        })
        .collect()
}

static EMPTY_CONSUMES: Vec<karnc::ast::ConsumesDecl> = Vec::new();

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(line: &str, doc: &str) -> Vec<String> {
        complete(line, doc, None)
            .into_iter()
            .map(|c| c.label)
            .collect()
    }

    #[test]
    fn consumes_target_suggests_units_including_karn() {
        // An adapter in the open doc plus the always-available `karn` surface.
        let doc = "adapter tokens {\n  binding \"./b.ts\"\n  capability Jwt { fn f() -> Effect[Int] }\n  provides Jwt = X\n}\n";
        let got = labels("  consumes ", doc);
        assert!(got.contains(&"karn".to_string()), "{got:?}");
        assert!(got.contains(&"tokens".to_string()), "{got:?}");
    }

    #[test]
    fn consumes_brace_suggests_that_units_capabilities() {
        let got = labels("  consumes karn { ", "context a.b\n");
        // The embedded `karn` surface exports these.
        assert!(got.contains(&"Clock".to_string()), "{got:?}");
        assert!(got.contains(&"Random".to_string()), "{got:?}");
        assert!(got.contains(&"Logger".to_string()), "{got:?}");
    }

    #[test]
    fn given_suggests_local_and_flattened_capabilities() {
        let doc = "context a.b\n\
                   consumes karn { Clock }\n\
                   capability Local { fn f() -> Effect[Int] }\n\
                   service s {\n\
                   on call() -> Effect[Int] given Clock {\n\
                   1\n\
                   }\n\
                   }\n";
        let got = labels("    on call() -> Effect[Int] given ", doc);
        assert!(got.contains(&"Clock".to_string()), "flattened: {got:?}");
        assert!(got.contains(&"Local".to_string()), "local: {got:?}");
    }

    #[test]
    fn no_completion_in_plain_position() {
        assert!(labels("  let x = ", "context a.b\n").is_empty());
    }

    #[test]
    fn consumes_with_as_is_not_a_target_completion() {
        // `consumes X as ` is aliasing, not target-name completion.
        assert!(!is_consumes_target("consumes platform.time as "));
        assert!(is_consumes_target("consumes platform"));
    }

    fn find<'a>(
        items: &'a [Completion],
        label: &str,
        kind: CompletionKind,
    ) -> Option<&'a Completion> {
        items.iter().find(|c| c.label == label && c.kind == kind)
    }

    #[test]
    fn type_annotation_suggests_builtins_surface_and_project_types() {
        let doc = "commons m {\n  type Order = { id: Int }\n}\n";
        let got = labels("  let x: ", doc);
        // Built-ins (with registry docs), the `karn`-surface transparent types,
        // and the project's own type declaration.
        for want in ["Int", "Option", "Result", "Effect", "List", "Map"] {
            assert!(got.contains(&want.to_string()), "built-in {want}: {got:?}");
        }
        assert!(got.contains(&"Uuid".to_string()), "surface: {got:?}");
        assert!(got.contains(&"Order".to_string()), "project: {got:?}");
    }

    #[test]
    fn return_type_and_type_args_are_type_positions() {
        assert!(is_type_position("  on call() -> "));
        assert!(is_type_position("  let x: Option["));
        assert!(is_type_position("  let x: Result[Int, "));
        // A partial type name being typed still counts.
        assert!(is_type_position("  -> Eff"));
    }

    #[test]
    fn list_literal_is_not_a_type_position() {
        // A bare `[` opening a list literal is an expression, not type args.
        assert!(!is_type_position("  let xs = ["));
        // And it yields no completion at all in slice 1.
        assert!(labels("  let xs = [", "context a.b\n").is_empty());
    }

    #[test]
    fn builtin_type_carries_its_registry_doc() {
        let items = complete("  let x: ", "context a.b\n", None);
        let int = find(&items, "Int", CompletionKind::Type).expect("Int present");
        assert_eq!(int.detail.as_deref(), keyword_doc("Int"));
        assert!(int.detail.is_some(), "Int should have a doc");
    }

    #[test]
    fn keyword_position_suggests_keywords_and_snippets() {
        let items = complete("  ", "context a.b\n", None);
        // Declaration/statement keywords, with docs.
        assert!(find(&items, "capability", CompletionKind::Keyword).is_some());
        assert!(find(&items, "fn", CompletionKind::Keyword).is_some());
        assert!(find(&items, "let", CompletionKind::Keyword).is_some());
        // Uppercase type/value names are *not* keyword-position candidates.
        assert!(find(&items, "Int", CompletionKind::Keyword).is_none());
        assert!(find(&items, "Some", CompletionKind::Keyword).is_none());
        // Snippets are offered alongside.
        let snip = find(&items, "service", CompletionKind::Snippet).expect("service snippet");
        let body = snip.insert_text.as_deref().unwrap_or("");
        assert!(body.contains("on call"), "snippet body: {body:?}");
        assert!(body.contains("${1"), "snippet tab stop: {body:?}");
    }

    #[test]
    fn keyword_position_fires_on_an_empty_line() {
        assert!(is_keyword_position(""));
        assert!(is_keyword_position("  cap"));
        assert!(!is_keyword_position("  let x ="));
        assert!(!is_keyword_position("  x: "));
        assert!(!complete("", "context a.b\n", None).is_empty());
    }
}
