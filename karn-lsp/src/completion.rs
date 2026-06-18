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
//! v0.30.1 slice 2 adds the **name-receiver `.`-member** context:
//!
//! - **`UpperIdent.<cursor>`** — where the receiver is a *name* (read from the
//!   line prefix), not a typed value: sum-type variants (`Color.Red`),
//!   refined/opaque `of`/`unsafe` constructors, capability operations, and
//!   built-in type statics (`Int.parse`/`Json.decode`). Value receivers
//!   (`list.map`) need the receiver's type and stay deferred (slice 3).
//!
//! Context detection is lexical (it must work mid-edit, when the buffer rarely
//! parses); candidates are semantic. Unit/type/capability/member enumeration
//! parses the project's `.karn` files (and the embedded `karn` surface) with
//! recovery, so it works even while the file the cursor sits in is mid-edit.
//! Built-ins and keywords come from the static `karnc` registries (`keywords`/
//! `builtin_names`/`firstparty`), never the index — first-party symbols aren't
//! indexed (the v0.28 finding). Value `.method`/`.field` and locals/params-in-
//! scope need receiver typing + a scope-at-offset query and are deferred to
//! slice 3.

use std::collections::BTreeSet;
use std::path::Path;

use karnc::ast::{CommonsItem, ExportKind, SourceUnit, TypeBody};
use karnc::checker::Ty;
use karnc::firstparty::{CLOUDFLARE_ADAPTER_SRC, KARN_ADAPTER_SRC};
use karnc::{kernel_methods, keywords, lexer, parser};

use crate::symbols::walk_karn_files;

/// What a candidate refers to — maps to an LSP `CompletionItemKind`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Unit,
    Capability,
    Type,
    Keyword,
    Snippet,
    /// A sum-type variant (`Color.Red`).
    Variant,
    /// A name-receiver member: a refined/opaque `of`/`unsafe` constructor, a
    /// capability operation, or a built-in type static (`Int.parse`).
    Member,
    /// A record field on a value receiver (`order.total`).
    Field,
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
    // 4. `UpperIdent.<cursor>` — name-receiver members: sum variants, refined/
    //    opaque `of`/`unsafe`, capability ops, or built-in type statics.
    if let Some(receiver) = member_receiver(line_prefix) {
        return member_candidates(&receiver, doc_text, src_root);
    }
    // 5. Type position (`: T`, `-> T`, `[ … ]` type args) — built-ins, the
    //    `karn`-surface transparent types, and project type declarations.
    if is_type_position(line_prefix) {
        return type_candidates(doc_text, src_root);
    }
    // 6. Keyword position (a bare word at a declaration/statement start) — the
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
pub fn is_keyword_position(line: &str) -> bool {
    line.trim().chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// The cursor sits where a **value** expression is expected — after `=`/`(`/`,`,
/// a `=>` lambda arrow, or a binary operator — so in-scope locals are offered
/// (v0.31, ADR 0064). Conservative: covers the common positions, excludes the
/// type arrow `->`. (The handler also offers locals at keyword position.)
pub fn is_expression_position(line: &str) -> bool {
    let head = line
        .trim_end_matches(|c: char| c.is_alphanumeric() || c == '_')
        .trim_end();
    if head.ends_with("->") {
        return false; // a return/param type, not a value
    }
    if head.ends_with("=>") {
        return true; // a lambda body
    }
    matches!(
        head.chars().last(),
        Some('=' | '(' | ',' | '[' | '+' | '-' | '*' | '/' | '<' | '>' | '&' | '|')
    )
}

/// `UpperIdent.<partial>` at the cursor → `Some("UpperIdent")` — a name
/// receiver whose members are statically enumerable (a sum/refined/opaque
/// type or a capability). Conservative: the receiver is a **single**
/// uppercase-initial identifier, not itself a `.`-qualified segment (so
/// `karn.cloudflare.` and `a.B.` are excluded) and not a number (so the
/// decimal `1.` is excluded). A lowercase `x.` is a *value* receiver — deferred
/// to slice 3 — and yields `None`.
fn member_receiver(line: &str) -> Option<String> {
    // Drop the partial member name being typed, then require a trailing dot.
    let head = line
        .trim_end_matches(|c: char| c.is_alphanumeric() || c == '_')
        .strip_suffix('.')?;
    // The receiver is the identifier immediately before that dot.
    let start = head
        .rfind(|c: char| !(c.is_alphanumeric() || c == '_'))
        .map_or(0, |i| i + 1);
    let recv = &head[start..];
    let first = recv.chars().next()?;
    if !first.is_ascii_uppercase() {
        return None;
    }
    // Reject a `.`-qualified receiver (`a.B.`): the char before it is a dot.
    if head[..start].ends_with('.') {
        return None;
    }
    Some(recv.to_string())
}

/// Built-in type statics — real language statics that are not user-declared, so
/// they come from this small table rather than the project parse. Covers the
/// numeric parse statics and the JSON codec (v0.22, ADRs 0048/0049), the
/// collection `empty` constructors (v0.20b), and `Effect.pure` (v0.5). The full
/// real set per ADR 0093 D2 — kept complete and drift-tested
/// (`builtin_statics_are_reachable`).
pub(crate) const BUILTIN_STATICS: &[(&str, &[(&str, &str)])] = &[
    ("Int", &[("parse", "parse(s: String) -> Option[Int]")]),
    ("Float", &[("parse", "parse(s: String) -> Option[Float]")]),
    (
        "Json",
        &[
            ("encode", "encode(value) -> String"),
            ("decode", "decode[T](s: String) -> Result[T, JsonError]"),
        ],
    ),
    ("List", &[("empty", "empty() -> List[T]")]),
    ("Map", &[("empty", "empty() -> Map[K, V]")]),
    ("Effect", &[("pure", "pure(value) -> Effect[T]")]),
];

/// Variants of a built-in sum type (`HttpResult`/`QueueResult`), sourced from
/// the AST variant registries so a new variant surfaces in completion for free
/// (ADR 0093 D2/G3). Empty for any other receiver.
fn builtin_sum_variants(receiver: &str) -> Vec<(String, String)> {
    match receiver {
        "HttpResult" => karnc::ast::HTTP_VARIANTS
            .iter()
            .map(|v| {
                (
                    v.name.to_string(),
                    format!("variant of `HttpResult` ({})", v.status),
                )
            })
            .collect(),
        "QueueResult" => karnc::ast::QUEUE_VARIANTS
            .iter()
            .map(|v| (v.name.to_string(), "variant of `QueueResult`".to_string()))
            .collect(),
        _ => Vec::new(),
    }
}

/// Members of a name receiver: built-in type statics, then built-in sum-type
/// variants, then — from the project and embedded-surface parse — project sum
/// variants, refined/opaque `of`/`unsafe`, or capability operations. Yields `[]`
/// when the receiver resolves to none of these (e.g. a plain `type X = Int`
/// alias or a record).
fn member_candidates(receiver: &str, doc_text: &str, src_root: Option<&Path>) -> Vec<Completion> {
    if let Some((_, statics)) = BUILTIN_STATICS.iter().find(|(name, _)| *name == receiver) {
        return statics
            .iter()
            .map(|(label, sig)| {
                Completion::item(*label, CompletionKind::Member, Some(sig.to_string()))
            })
            .collect();
    }
    let mut out: Vec<Completion> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    // Built-in sum types (`HttpResult`/`QueueResult`) — variants from the AST
    // registry, on the same name-receiver path as project sums (ADR 0093 G3).
    for (label, detail) in builtin_sum_variants(receiver) {
        if seen.insert(label.clone()) {
            out.push(Completion::item(
                label,
                CompletionKind::Variant,
                Some(detail),
            ));
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
            match item {
                CommonsItem::Type(t) if t.name.name == receiver => match &t.body {
                    karnc::ast::TypeBody::Sum(s) => {
                        for v in &s.variants {
                            if seen.insert(v.name.name.clone()) {
                                out.push(Completion::item(
                                    v.name.name.clone(),
                                    CompletionKind::Variant,
                                    Some(format!("variant of `{receiver}`")),
                                ));
                            }
                        }
                    }
                    karnc::ast::TypeBody::Refined { .. } | karnc::ast::TypeBody::Opaque { .. } => {
                        for (label, sig) in [
                            (
                                "of",
                                format!("of(value) -> Result[{receiver}, ValidationError]"),
                            ),
                            ("unsafe", format!("unsafe(value) -> {receiver}")),
                        ] {
                            if seen.insert(label.to_string()) {
                                out.push(Completion::item(
                                    label,
                                    CompletionKind::Member,
                                    Some(sig),
                                ));
                            }
                        }
                    }
                    // A plain alias (`type X = Int`) or a record has no
                    // name-receiver members — record fields are value-receiver
                    // (slice 3).
                    _ => {}
                },
                CommonsItem::Capability(c) if c.name.name == receiver => {
                    for op in &c.ops {
                        if seen.insert(op.name.name.clone()) {
                            let params = op
                                .params
                                .iter()
                                .map(|p| p.name.name.as_str())
                                .collect::<Vec<_>>()
                                .join(", ");
                            out.push(Completion::item(
                                op.name.name.clone(),
                                CompletionKind::Member,
                                Some(format!(
                                    "{}({params}) — operation of `{receiver}`",
                                    op.name.name
                                )),
                            ));
                        }
                    }
                }
                _ => {}
            }
        }
    });
    out
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
pub(crate) fn for_each_unit(
    doc_text: &str,
    src_root: Option<&Path>,
    mut f: impl FnMut(&SourceUnit),
) {
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

// -- Value-receiver `.method`/`.field` (slice 3, ADR 0063) --

/// If the cursor (byte `offset` into `text`) sits just after a **lowercase**
/// `receiver.`(`partial`) — a *value* receiver — return the buffer **rewritten**
/// so the receiver is a complete expression (the trailing `.partial` dropped,
/// so the file parses) and the byte offset of the receiver to type. Returns
/// `None` for an uppercase name receiver (slice 2), a decimal `1.`, or a
/// `.`-qualified segment.
///
/// The rewrite is the spike's fix for the mid-edit parse: a bare `email.`
/// cascades and loses the receiver, but `email` (dot dropped) types cleanly.
pub fn value_receiver_rewrite(text: &str, offset: usize) -> Option<(String, usize)> {
    let prefix = text.get(..offset)?;
    let head = prefix
        .trim_end_matches(|c: char| c.is_alphanumeric() || c == '_')
        .strip_suffix('.')?;
    let start = head
        .rfind(|c: char| !(c.is_alphanumeric() || c == '_'))
        .map_or(0, |i| i + 1);
    let recv = &head[start..];
    let first = recv.chars().next()?;
    if !(first.is_ascii_lowercase() || first == '_') {
        return None; // uppercase = name receiver (slice 2); a digit = a decimal
    }
    if head[..start].ends_with('.') {
        return None; // a `.`-qualified segment, not a bare value receiver
    }
    let dot = head.len(); // the receiver ends here; the dot was the next byte
    let rewritten = format!("{}{}", &text[..dot], &text[offset..]);
    Some((rewritten, dot.saturating_sub(1)))
}

/// The members of a typed value receiver: the built-in kernel methods of its
/// type (from the enumerable registry) plus, for a record, its fields.
pub fn value_member_candidates(
    ty: &Ty,
    doc_text: &str,
    src_root: Option<&Path>,
) -> Vec<Completion> {
    let mut out: Vec<Completion> = kernel_methods::methods_for(ty)
        .iter()
        .map(|km| {
            Completion::item(
                km.name,
                CompletionKind::Member,
                Some(km.signature.to_string()),
            )
        })
        .collect();
    // Record fields — resolve the receiver's named type to its declaration.
    if let Ty::Named { name, .. } = ty {
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for_each_unit(doc_text, src_root, |unit| {
            let items = match unit {
                SourceUnit::Commons(c) => &c.items,
                SourceUnit::Context(c) => &c.items,
                SourceUnit::Adapter(a) => &a.items,
                _ => return,
            };
            for item in items {
                if let CommonsItem::Type(t) = item
                    && &t.name.name == name
                    && let TypeBody::Record(r) = &t.body
                {
                    for f in &r.fields {
                        if seen.insert(f.name.name.clone()) {
                            out.push(Completion::item(
                                f.name.name.clone(),
                                CompletionKind::Field,
                                Some(format!("field of `{name}`")),
                            ));
                        }
                    }
                }
            }
        });
    }
    out
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
    fn member_completion_reaches_inside_an_interpolation_hole() {
        // v0.43: a `Type.`/`Cap.` receiver inside a `\(…)` hole completes just
        // as it does in bare expression position — context detection is purely
        // lexical, so the surrounding string and `\(` do not interfere.
        let doc = "context a.b\n  capability Timer { fn now() -> Effect[Int] }\n";
        let in_hole = complete("    \"the time is \\(Timer.", doc, None);
        assert!(
            find(&in_hole, "now", CompletionKind::Member).is_some(),
            "capability op not offered inside a hole: {:?}",
            in_hole.iter().map(|c| &c.label).collect::<Vec<_>>()
        );
        // A built-in static receiver works inside a hole too.
        let statics = complete("  \"n=\\(Int.", "context a.b\n", None);
        assert!(find(&statics, "parse", CompletionKind::Member).is_some());
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

    #[test]
    fn member_receiver_is_a_single_upper_ident_before_a_dot() {
        assert_eq!(member_receiver("  Color."), Some("Color".to_string()));
        assert_eq!(
            member_receiver("  let e = Email.o"),
            Some("Email".to_string())
        );
        assert_eq!(member_receiver("  x."), None); // lowercase = value receiver (slice 3)
        assert_eq!(member_receiver("  1."), None); // decimal literal, not a member access
        assert_eq!(member_receiver("  a.B."), None); // `.`-qualified segment
        assert_eq!(member_receiver("  Color"), None); // no dot yet
    }

    #[test]
    fn sum_member_suggests_variants() {
        let doc = "commons m {\n  type Color = enum { Red, Green, Blue }\n}\n";
        let items = complete("  let c = Color.", doc, None);
        for v in ["Red", "Green", "Blue"] {
            assert!(
                find(&items, v, CompletionKind::Variant).is_some(),
                "variant {v}: {:?}",
                items.iter().map(|c| &c.label).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn refined_and_plain_alias_members_are_of_and_unsafe() {
        // A refinement-bearing type…
        let doc = "commons m {\n  type Email = String where NonEmpty\n}\n";
        let items = complete("  Email.", doc, None);
        assert!(find(&items, "of", CompletionKind::Member).is_some());
        assert!(find(&items, "unsafe", CompletionKind::Member).is_some());
        // …and a plain alias `type Id = Int` is *also* branded (the emitter
        // emits Id.of/Id.unsafe for every Refined body, refinement or not).
        let doc = "commons m {\n  type Id = Int\n}\n";
        assert!(find(&complete("  Id.", doc, None), "of", CompletionKind::Member).is_some());
    }

    #[test]
    fn capability_member_suggests_ops() {
        let doc = "context a.b\n  capability Timer { fn now() -> Effect[Int] }\n";
        let items = complete("    Timer.", doc, None);
        assert!(
            find(&items, "now", CompletionKind::Member).is_some(),
            "{:?}",
            items.iter().map(|c| &c.label).collect::<Vec<_>>()
        );
    }

    #[test]
    fn builtin_type_statics_are_offered() {
        assert!(
            find(
                &complete("  Int.", "context a.b\n", None),
                "parse",
                CompletionKind::Member
            )
            .is_some()
        );
        let j = complete("  Json.", "context a.b\n", None);
        assert!(find(&j, "encode", CompletionKind::Member).is_some());
        assert!(find(&j, "decode", CompletionKind::Member).is_some());
    }

    #[test]
    fn builtin_sum_variants_are_complete() {
        // ADR 0093 D5/G3: every built-in sum variant in the AST registry must
        // surface on its name receiver. Registry-driven — adding an
        // `HttpResult`/`QueueResult` variant must appear in completion or this
        // fails (the standing drift guard, mirroring `kernel_registry`).
        let http: Vec<&str> = karnc::ast::HTTP_VARIANTS.iter().map(|v| v.name).collect();
        let queue: Vec<&str> = karnc::ast::QUEUE_VARIANTS.iter().map(|v| v.name).collect();
        for (recv, names) in [("HttpResult", http), ("QueueResult", queue)] {
            let items = complete(&format!("  {recv}."), "context a.b\n", None);
            for name in names {
                assert!(
                    find(&items, name, CompletionKind::Variant).is_some(),
                    "{recv}.{name} missing: {:?}",
                    items.iter().map(|c| &c.label).collect::<Vec<_>>()
                );
            }
        }
    }

    #[test]
    fn builtin_statics_are_reachable() {
        // ADR 0093 D5/G2: every BUILTIN_STATICS entry is reachable through the
        // name-receiver context — exercises the member_receiver→member_candidates
        // wiring for each receiver (e.g. that `Effect.`/`List.` are recognised).
        for &(recv, members) in BUILTIN_STATICS {
            let items = complete(&format!("  {recv}."), "context a.b\n", None);
            for &(member, _) in members {
                assert!(
                    find(&items, member, CompletionKind::Member).is_some(),
                    "{recv}.{member} unreachable: {:?}",
                    items.iter().map(|c| &c.label).collect::<Vec<_>>()
                );
            }
        }
        // The slice-1 additions specifically — guards against a table regression
        // (the loop above can't catch an entry being deleted).
        for (recv, member) in [("List", "empty"), ("Map", "empty"), ("Effect", "pure")] {
            let items = complete(&format!("  {recv}."), "context a.b\n", None);
            assert!(
                find(&items, member, CompletionKind::Member).is_some(),
                "{recv}.{member} missing from the statics table"
            );
        }
    }

    #[test]
    fn record_value_and_decimal_receivers_yield_nothing() {
        // A record type has no name-receiver members (fields are value-receiver).
        let doc = "commons m {\n  type Point = { x: Int }\n}\n";
        assert!(complete("  Point.", doc, None).is_empty(), "record");
        // A lowercase value receiver is deferred to slice 3.
        assert!(complete("  let p = q.", doc, None).is_empty(), "value");
        // A decimal literal is not a member access.
        assert!(complete("  let n = 1.", doc, None).is_empty(), "decimal");
    }

    #[test]
    fn value_receiver_rewrite_drops_the_dot_for_lowercase_receivers() {
        let text = "  let x = email.\n";
        let offset = text.find('.').unwrap() + 1; // just after the dot
        let (rewritten, recv) = value_receiver_rewrite(text, offset).expect("value receiver");
        assert_eq!(
            rewritten, "  let x = email\n",
            "the trailing dot is dropped"
        );
        assert!(
            text.get(recv..=recv).is_some_and(|c| c == "l"),
            "the receiver offset lands inside `email`"
        );
        // A partial member is dropped too.
        let text2 = "  let x = email.ma\n";
        let off2 = text2.find(".ma").unwrap() + 3;
        assert_eq!(
            value_receiver_rewrite(text2, off2).map(|(r, _)| r),
            Some("  let x = email\n".to_string())
        );
        // Uppercase (name receiver, slice 2), decimal, and no-dot yield None.
        assert!(value_receiver_rewrite("  Email.", 8).is_none());
        assert!(value_receiver_rewrite("  let n = 1.", 12).is_none());
        assert!(value_receiver_rewrite("  email", 7).is_none());
    }

    #[test]
    fn value_member_candidates_lists_kernel_methods() {
        use karnc::ast::BaseType;
        let list = Ty::List(Box::new(Ty::Base(BaseType::Int)));
        let items = value_member_candidates(&list, "context a.b\n", None);
        assert!(find(&items, "fold", CompletionKind::Member).is_some());
        assert!(find(&items, "get", CompletionKind::Member).is_some());

        let string = Ty::Base(BaseType::String);
        let items = value_member_candidates(&string, "context a.b\n", None);
        assert!(find(&items, "split", CompletionKind::Member).is_some());
        assert!(find(&items, "trim", CompletionKind::Member).is_some());
    }

    #[test]
    fn expression_position_offers_locals() {
        // Value-expecting positions (locals offered).
        assert!(is_expression_position("  let y = "));
        assert!(is_expression_position("  let y = a + lo")); // after a binary op
        assert!(is_expression_position("  f("));
        assert!(is_expression_position("  g(a, "));
        assert!(is_expression_position("  xs.fold(0, (acc, x) => ac")); // lambda body
        // `let y = foo` is still a value position (you're typing the value).
        assert!(is_expression_position("  let y = foo"));
        // Not value positions.
        assert!(!is_expression_position("  let y: ")); // type annotation
        assert!(!is_expression_position("  on call() -> ")); // return type
        assert!(!is_expression_position("  tot")); // bare line start (keyword position covers it)
    }

    #[test]
    fn value_member_candidates_lists_record_fields() {
        use karnc::checker::NamedKind;
        let order = Ty::Named {
            name: "Order".to_string(),
            kind: NamedKind::Record,
        };
        let doc = "commons m {\n  type Order = { id: Int, total: Int }\n}\n";
        let items = value_member_candidates(&order, doc, None);
        assert!(
            find(&items, "id", CompletionKind::Field).is_some(),
            "{items:?}",
            items = items.iter().map(|c| &c.label).collect::<Vec<_>>()
        );
        assert!(find(&items, "total", CompletionKind::Field).is_some());
    }
}
