//! Completion for the cursor, keyed off the line up to it.
//!
//! The surface is the canonical *cursor context × candidate-kind* matrix fixed
//! by ADR 0093 (`design/decisions/0093-completion-surface-contract.md`), spec'd
//! at `design/bynk-lsp-spec.md` §3.15. [`complete`] dispatches the six contexts
//! it can serve purely (no analysis cache):
//!
//! - `consumes <prefix>` / `consumes U { … }` / `given …` — consumable units and
//!   in-scope capabilities (v0.17);
//! - **type position** (`: T`, `-> T`, inside `[ … ]` type args) — built-in
//!   types, the `bynk`-surface transparent types, and project `type` decls;
//! - **keyword position** (a bare word at a declaration/statement start) — the
//!   reserved keywords (with registry docs) and declaration snippets;
//! - **name-receiver `UpperIdent.`** — sum variants (project + built-in
//!   `HttpResult`/`QueueResult`), refined/opaque `of`/`unsafe`, capability ops,
//!   and built-in type statics (`Int.parse`/`List.empty`/`Effect.pure`/…);
//! - **expression position** (after `=`/`(`/`,`/`=>`/an operator) — the value
//!   constructors (`Ok`/`Some`/`true`/…), in-scope type names, and in-scope free
//!   functions (the current unit's own `fn`s + `uses`-imported stdlib/project
//!   combinators, gated on the `uses` set) (ADR 0093 D3).
//!
//! Two further contexts need the analysis overlay and so live handler-side
//! (`main.rs`): **value-receiver `lower.`** members (kernel methods + record
//! fields) and **in-scope locals/params**. They depend on the analysis overlay
//! (the boundary is ADR 0093 D4), but since slice 4 (ADR 0094) it is
//! error-tolerant: best-effort partial types are recorded even on a broken
//! buffer, so they no longer go silent on an unrelated error. Items also carry a
//! one-line `detail` eagerly; the richer `documentation` is filled in lazily by
//! `completionItem/resolve`, handler-side (slice 5).
//!
//! Context detection is lexical (it must work mid-edit, when the buffer rarely
//! parses); candidates are semantic. Unit/type/capability/member enumeration
//! parses the project's `.bynk` files (and the embedded `bynk` surface) with
//! recovery, so it works even while the file the cursor sits in is mid-edit.
//! Built-ins, keywords, and constructors come from the static `bynkc` registries
//! (`keywords`/`builtin_names`/`firstparty`/`ast`), never the index — first-party
//! symbols aren't indexed (the v0.28 finding); the project parse supplies only
//! *project* symbols.

use std::collections::BTreeSet;
use std::path::Path;

use bynk_check::checker::Ty;
use bynk_check::firstparty::{
    BYNK_ADAPTER_SRC, BYNK_LIST_SRC, BYNK_MAP_SRC, BYNK_STRING_SRC, CLOUDFLARE_ADAPTER_SRC,
};
use bynk_check::kernel_methods;
use bynk_syntax::ast::{CommonsItem, ExportKind, FnName, SourceUnit, TypeBody, UsesDecl};
use bynk_syntax::{keywords, lexer, parser};

use crate::symbols::{type_ref_str, walk_bynk_files};

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
    /// A value constructor at expression position (`Ok`/`Some`/`true`).
    Constructor,
    /// A free function in scope at expression position — the current unit's own
    /// top-level `fn`s and the `uses`-imported stdlib/project combinators.
    Function,
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
    pub(crate) fn item(
        label: impl Into<String>,
        kind: CompletionKind,
        detail: Option<String>,
    ) -> Self {
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
    // v0.124 (slice 3): the non-keyword clause/construction contexts, before the
    // generic type/keyword/expression cells they would otherwise fall into.
    // 4a. `Type { <cursor>` — record field names on construction.
    if let Some(recv) = record_construction_receiver(line_prefix) {
        let fields = record_field_names(&recv, doc_text, src_root);
        if !fields.is_empty() {
            return fields;
        }
    }
    // 4b. `from <cursor>` — the service protocols.
    if after_clause_keyword(line_prefix, "from") {
        return protocol_candidates();
    }
    // 4c. `on <cursor>` — the handler kinds.
    if after_clause_keyword(line_prefix, "on") {
        return handler_kind_candidates();
    }
    // 4d. `by <cursor>` — the project's actor names.
    if after_clause_keyword(line_prefix, "by") {
        return actor_candidates(doc_text, src_root);
    }
    // 4e. `exports <cursor>` — the export kinds (adapter).
    if after_clause_keyword(line_prefix, "exports") {
        return export_kind_candidates();
    }
    // 4f. `provides <cursor>` — the in-scope capabilities to implement.
    if after_clause_keyword(line_prefix, "provides") {
        return in_scope_capabilities(doc_text, src_root);
    }
    // 5. Type position (`: T`, `-> T`, `[ … ]` type args) — built-ins, the
    //    `bynk`-surface transparent types, and project type declarations.
    if is_type_position(line_prefix) {
        return type_candidates(doc_text, src_root);
    }
    // 6. Keyword position (a bare word at a declaration/statement start) — the
    //    reserved keywords plus declaration snippets.
    if is_keyword_position(line_prefix) {
        return keyword_and_snippet_candidates();
    }
    // 7. Expression position (after `=`/`(`/`,`/`=>`/a binary operator) — a value
    //    starts here: the constructor keywords + in-scope type names. In-scope
    //    locals/params (and, from slice 3, free functions) are appended
    //    handler-side, where the analysis cache lives (ADR 0093 D3).
    if is_expression_position(line_prefix) {
        return expression_candidates(doc_text, src_root);
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
/// `bynk.cloudflare.` and `a.B.` are excluded) and not a number (so the
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

/// v0.124 (slice 3): the cursor is at a field-*name* position of a record
/// construction — inside an unclosed `{` opened immediately after an
/// uppercase-initial type name (`Order { <cursor>` / `Order { id: 1, <cursor>`),
/// with the current field segment not yet past its `:` (a field *type*
/// position, left to [`is_type_position`]). Returns the record type name.
fn record_construction_receiver(line: &str) -> Option<String> {
    // The innermost `{` still open at the cursor.
    let bytes = line.as_bytes();
    let mut depth = 0i32;
    let mut open = None;
    for i in (0..bytes.len()).rev() {
        match bytes[i] {
            b'}' => depth += 1,
            b'{' => {
                if depth == 0 {
                    open = Some(i);
                    break;
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    let open = open?;
    // Only at a name position: the current field (since the last comma) has no
    // `:` yet — else the cursor is in that field's type.
    let current = line[open + 1..].rsplit(',').next().unwrap_or("");
    if current.contains(':') {
        return None;
    }
    // The receiver is the uppercase-initial identifier immediately before `{`.
    let head = line[..open].trim_end();
    let start = head
        .rfind(|c: char| !(c.is_alphanumeric() || c == '_'))
        .map_or(0, |i| i + 1);
    let recv = &head[start..];
    if recv.chars().next()?.is_ascii_uppercase() {
        Some(recv.to_string())
    } else {
        None
    }
}

/// v0.124 (slice 3): the cursor is completing the argument to a leading clause
/// keyword — `from`/`on`/`by`/`exports`/`provides <cursor>` — with only a
/// partial identifier typed after the keyword. `kw` must be a standalone word
/// (line start or whitespace before it), so a field named `from` or the `on`
/// inside `session` does not trigger it.
fn after_clause_keyword(line: &str, kw: &str) -> bool {
    let Some(idx) = line.rfind(kw) else {
        return false;
    };
    if !line[..idx]
        .chars()
        .last()
        .map(char::is_whitespace)
        .unwrap_or(true)
    {
        return false;
    }
    let after = &line[idx + kw.len()..];
    after.starts_with(char::is_whitespace)
        && after
            .trim_start()
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_')
}

/// v0.124 (slice 3): the cursor sits in a contract-clause predicate —
/// `requires <name>: <cursor>` or `ensures <name>: <cursor>` — where the
/// enclosing function's parameters (and, for an `ensures`, `result`) are in
/// scope. Returns `Some(is_ensures)`; the parameters themselves are resolved
/// handler-side from the enclosing `fn` (needs the cursor offset).
pub(crate) fn contract_clause_kind(line: &str) -> Option<bool> {
    let colon = line.rfind(':')?;
    let clause = line[..colon].trim();
    for (kw, is_ensures) in [("requires", false), ("ensures", true)] {
        if let Some(rest) = clause.strip_prefix(kw) {
            let rest = rest.trim();
            if !rest.is_empty() && rest.chars().all(|c| c.is_alphanumeric() || c == '_') {
                return Some(is_ensures);
            }
        }
    }
    None
}

/// The record fields of a project (or embedded-surface) type named `name`, as
/// field-name completions — the construction-position half of what
/// [`value_member_candidates`] offers on a value receiver.
fn record_field_names(name: &str, doc_text: &str, src_root: Option<&Path>) -> Vec<Completion> {
    let mut out: Vec<Completion> = Vec::new();
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
                && t.name.name == name
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
    out
}

/// v0.124 (slice 3): the variants of a project (or embedded-surface) sum type
/// named `name`, as pattern completions — the `is`/`match` candidate set once
/// the scrutinee's type is known (resolved handler-side from `expr_types`).
/// `pub(crate)` so the completion handler can offer them at an `is` position.
pub(crate) fn sum_type_variants(
    name: &str,
    doc_text: &str,
    src_root: Option<&Path>,
) -> Vec<Completion> {
    let mut out: Vec<Completion> = Vec::new();
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
                && t.name.name == name
                && let TypeBody::Sum(s) = &t.body
            {
                for v in &s.variants {
                    if seen.insert(v.name.name.clone()) {
                        out.push(Completion::item(
                            v.name.name.clone(),
                            CompletionKind::Variant,
                            Some(format!("variant of `{name}`")),
                        ));
                    }
                }
            }
        }
    });
    out
}

/// The service protocols offerable after `from`.
fn protocol_candidates() -> Vec<Completion> {
    ["http", "cron", "queue", "WebSocket"]
        .into_iter()
        .map(|p| Completion::item(p, CompletionKind::Keyword, Some("service protocol".into())))
        .collect()
}

/// The handler kinds offerable after `on`.
fn handler_kind_candidates() -> Vec<Completion> {
    [
        "call", "GET", "POST", "PUT", "PATCH", "DELETE", "schedule", "message", "open", "close",
    ]
    .into_iter()
    .map(|k| Completion::item(k, CompletionKind::Keyword, Some("handler kind".into())))
    .collect()
}

/// The export kinds offerable after `exports` (adapter).
fn export_kind_candidates() -> Vec<Completion> {
    ["capability", "transparent", "opaque"]
        .into_iter()
        .map(|k| Completion::item(k, CompletionKind::Keyword, Some("export kind".into())))
        .collect()
}

/// The project's `actor` names, offerable after `by`.
fn actor_candidates(doc_text: &str, src_root: Option<&Path>) -> Vec<Completion> {
    let mut out: Vec<Completion> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for_each_unit(doc_text, src_root, |unit| {
        let items = match unit {
            SourceUnit::Commons(c) => &c.items,
            SourceUnit::Context(c) => &c.items,
            SourceUnit::Adapter(a) => &a.items,
            _ => return,
        };
        for item in items {
            if let CommonsItem::Actor(a) = item
                && seen.insert(a.name.name.clone())
            {
                out.push(Completion::item(
                    a.name.name.clone(),
                    CompletionKind::Type,
                    Some("actor".into()),
                ));
            }
        }
    });
    out
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
    (
        "Bytes",
        &[
            ("fromUtf8", "fromUtf8(s: String) -> Bytes"),
            ("fromBase64", "fromBase64(s: String) -> Option[Bytes]"),
            ("empty", "empty() -> Bytes"),
        ],
    ),
];

/// Variants of a built-in sum type (`HttpResult`/`QueueResult`), sourced from
/// the AST variant registries so a new variant surfaces in completion for free
/// (ADR 0093 D2/G3). Empty for any other receiver.
fn builtin_sum_variants(receiver: &str) -> Vec<(String, String)> {
    match receiver {
        "HttpResult" => bynk_syntax::ast::HTTP_VARIANTS
            .iter()
            .map(|v| {
                (
                    v.name.to_string(),
                    format!("variant of `HttpResult` ({})", v.status),
                )
            })
            .collect(),
        "QueueResult" => bynk_syntax::ast::QUEUE_VARIANTS
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
                    bynk_syntax::ast::TypeBody::Sum(s) => {
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
                    bynk_syntax::ast::TypeBody::Refined { .. }
                    | bynk_syntax::ast::TypeBody::Opaque { .. } => {
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
                            // Typed signature (params + return), the same
                            // `type_ref_str` rendering hover/signature help use —
                            // not bare param names (slice 5 detail polish).
                            let params = op
                                .params
                                .iter()
                                .map(|p| format!("{}: {}", p.name.name, type_ref_str(&p.type_ref)))
                                .collect::<Vec<_>>()
                                .join(", ");
                            out.push(Completion::item(
                                op.name.name.clone(),
                                CompletionKind::Member,
                                Some(format!(
                                    "{}({params}) -> {} — operation of `{receiver}`",
                                    op.name.name,
                                    type_ref_str(&op.return_type)
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
    bynk_check::builtin_names::types::INT,
    "Bool",
    bynk_check::builtin_names::types::FLOAT,
    "String",
    "Option",
    "Result",
    "Effect",
    bynk_check::builtin_names::types::LIST,
    bynk_check::builtin_names::types::MAP,
];

/// Declaration snippets (`CompletionItemKind::SNIPPET`), as LSP snippet bodies.
/// `pub(crate)` so `tests/scaffolds_compile.rs` (ADR 0157) can enumerate them.
pub(crate) const SNIPPETS: &[(&str, &str)] = &[
    // -- Units --
    ("context", "context ${1:name} {\n\t$0\n}"),
    ("commons", "commons ${1:my.lib}\n\n$0"),
    (
        "adapter",
        "adapter ${1:name} {\n\tbinding \"${2:./module}\"\n\t$0\n}",
    ),
    // -- Unit-header clauses --
    ("uses", "uses ${1:module}"),
    ("consumes", "consumes ${1:bynk} { ${2:Random} }"),
    // -- Types --
    (
        "type record",
        "type ${1:Name} = {\n\t${2:field}: ${3:Int},\n}",
    ),
    ("type enum", "type ${1:Name} = enum {\n\t${2:Variant},\n}"),
    (
        "type refined",
        "type ${1:Name} = ${2:String} where ${3:MinLength(1)}",
    ),
    (
        "type opaque",
        "type ${1:Name} = opaque ${2:Int} where ${3:NonNegative}",
    ),
    // -- Functions --
    (
        "fn",
        "fn ${1:name}(${2:x}: ${3:Int}) -> ${4:Int} {\n\t$0\n}",
    ),
    (
        "fn contract",
        "fn ${1:name}(${2:x}: ${3:Int}) -> ${4:Int}\n\trequires ${5:in_range}: ${6:x >= 0}\n\tensures ${7:non_negative}: ${8:result >= 0}\n{\n\t$0\n}",
    ),
    // -- Capabilities & providers --
    (
        "capability",
        "capability ${1:Name} {\n\tfn ${2:op}() -> Effect[${3:Unit}]\n}",
    ),
    (
        "provides",
        "provides ${1:Cap} = ${2:Impl} {\n\tfn ${3:op}(${4}) -> Effect[${5:()}] {\n\t\tEffect.pure(${6:()})\n\t}\n}",
    ),
    // -- Actors & agents --
    (
        "actor",
        "actor ${1:Name} { auth = ${2:Bearer(secret = \"AUTH_JWT_SECRET\")}, identity = ${3:UserId} }",
    ),
    (
        "agent",
        "agent ${1:Name} {\n\tkey ${2:id}: ${3:String}\n\n\tstore ${4:status}: Cell[${5:Int}] = ${6:0}\n\n\tinvariant ${7:non_negative}: ${8:status >= 0}\n\n\ttransition ${9:monotonic}: ${10:new.status >= old.status}\n\n\ton call ${11:op}(${12}) -> Effect[Result[${13:()}, String]] {\n\t\tOk(${14:()})\n\t}\n}",
    ),
    // -- Services & handlers --
    (
        "service",
        "service ${1:name} {\n\ton call(${2}) -> Effect[${3:Unit}] {\n\t\t$0\n\t}\n}",
    ),
    ("on call", "on call(${1}) -> Effect[${2:Unit}] {\n\t$0\n}"),
    (
        "on http",
        "on ${1|GET,POST,PUT,DELETE,PATCH|}(\"${2:/path}\") (${3:body}: ${4:Req}) -> Effect[HttpResult[${5:Res}]] given ${6:Cap} {\n\t$0\n}",
    ),
    (
        "on cron",
        "on schedule(\"${1:0 * * * *}\") () -> Effect[Result[(), String]] {\n\t$0\n\tOk(())\n}",
    ),
    // -- Tests --
    (
        "suite",
        "suite ${1:target}\n\ncase \"${2:it works}\" {\n\tlet ${3:actual} = ${4:0}\n\texpect ${5:actual == 0}\n}",
    ),
    (
        "property",
        "property \"${1:invariant holds}\" {\n\tfor all ${2:x}: ${3:Int} {\n\t\texpect ${4:x == x}\n\t}\n}",
    ),
];

/// The value constructors offered at expression position (ADR 0093 D3) — the
/// closed set of `Result`/`Option` variant constructors and the boolean
/// literals. A value expression can begin with any of these; their docs reuse
/// the `keywords` registry (one source of truth).
const CONSTRUCTORS: &[&str] = &["Ok", "Err", "Some", "None", "true", "false"];

/// Expression-position candidates: the value constructors plus in-scope type
/// names (the entry to a static call like `Int.parse` or a record construction
/// like `Order { … }`). In-scope values — locals/params, and from slice 3 free
/// functions — are appended by the handler, which owns the analysis cache, so
/// they are not produced here (ADR 0093 D3).
fn expression_candidates(doc_text: &str, src_root: Option<&Path>) -> Vec<Completion> {
    let mut out: Vec<Completion> = CONSTRUCTORS
        .iter()
        .map(|&name| {
            Completion::item(
                name,
                CompletionKind::Constructor,
                keyword_doc(name).map(str::to_string),
            )
        })
        .collect();
    // Type names are valid here too (static receiver / record construction); the
    // `Type.` member context (slice 1) takes over once the user types the dot.
    out.extend(type_candidates(doc_text, src_root));
    // In-scope free functions — the current unit's own `fn`s and the combinators
    // of every `uses`-imported module (project + stdlib) — ADR 0093 D3 / G5.
    out.extend(free_function_candidates(doc_text, src_root));
    out
}

/// A unit's top-level items and its `uses` clauses, for the kinds that carry
/// free functions. Service/other units contribute neither.
fn unit_items_and_uses(unit: &SourceUnit) -> (&[CommonsItem], &[UsesDecl]) {
    match unit {
        SourceUnit::Commons(c) => (&c.items, &c.uses),
        SourceUnit::Context(c) => (&c.items, &c.uses),
        SourceUnit::Adapter(a) => (&a.items, &a.uses),
        _ => (&[], &[]),
    }
}

/// The qualified name of the unit the cursor's document declares, via a recovery
/// parse (the header survives a mid-edit body). `None` for a headerless fragment
/// that names no unit.
fn current_unit_name(doc_text: &str) -> Option<String> {
    let tokens = lexer::tokenize(doc_text).ok()?;
    let (unit, _errs) = parser::parse_unit_with_recovery(&tokens, doc_text);
    Some(unit?.name().joined())
}

/// Render a free function's signature for the completion detail, the same way
/// hover and signature help do (`symbols::type_ref_str`) — one format, never
/// divergent. Mirrors signature help: no generic-parameter list.
fn free_fn_signature(name: &str, f: &bynk_syntax::ast::FnDecl) -> String {
    let params = f
        .params
        .iter()
        .map(|p| format!("{}: {}", p.name.name, type_ref_str(&p.type_ref)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{name}({params}) -> {}", type_ref_str(&f.return_type))
}

/// Free-function candidates at expression position: the current unit's own
/// top-level `fn`s plus the free `fn`s of every `uses`-imported module (project
/// commons and the embedded stdlib). Gated on the `uses` set so a combinator is
/// offered only where it is actually in scope (ADR 0093 D3 / G5).
fn free_function_candidates(doc_text: &str, src_root: Option<&Path>) -> Vec<Completion> {
    let Some(current) = current_unit_name(doc_text) else {
        return Vec::new();
    };
    // One parse pass: collect each unit's name, its free `fn`s (name + signature),
    // and its `uses` targets.
    struct UnitFns {
        name: String,
        fns: Vec<(String, String)>,
        uses: Vec<String>,
    }
    let mut units: Vec<UnitFns> = Vec::new();
    for_each_unit(doc_text, src_root, |unit| {
        let (items, uses) = unit_items_and_uses(unit);
        let fns = items
            .iter()
            .filter_map(|it| match it {
                CommonsItem::Fn(f) => match &f.name {
                    FnName::Free(id) => Some((id.name.clone(), free_fn_signature(&id.name, f))),
                    FnName::Method { .. } => None,
                },
                _ => None,
            })
            .collect();
        units.push(UnitFns {
            name: unit.name().joined(),
            fns,
            uses: uses.iter().map(|u| u.target.joined()).collect(),
        });
    });
    // The import scope: the `uses` targets of every unit sharing the current name
    // (a unit may span files, so union them).
    let mut imported: BTreeSet<String> = BTreeSet::new();
    for u in &units {
        if u.name == current {
            imported.extend(u.uses.iter().cloned());
        }
    }
    // Offer the current unit's own fns and the fns of each imported module.
    let mut out: Vec<Completion> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for u in &units {
        let own = u.name == current;
        if !own && !imported.contains(&u.name) {
            continue;
        }
        let origin = if own { "this unit" } else { u.name.as_str() };
        for (name, sig) in &u.fns {
            if seen.insert(name.clone()) {
                out.push(Completion::item(
                    name.clone(),
                    CompletionKind::Function,
                    Some(format!("{sig} — `{origin}`")),
                ));
            }
        }
    }
    out
}

/// The one-line doc for a name in the `keywords` registry, if present.
/// `pub(crate)` so hover's bare-keyword fallback (ADR 0156) can reuse it —
/// completion and hover render the same doc, never a parallel copy.
pub(crate) fn keyword_doc(word: &str) -> Option<&'static str> {
    keywords::KEYWORDS
        .iter()
        .find(|k| k.word == word)
        .map(|k| k.meaning)
}

/// Type-position candidates: built-in types (with registry docs), then every
/// `type` declaration found in the project sources and the embedded `bynk`
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

// -- Enumeration (parse project sources + the embedded `bynk` surface) --

/// Parse every project unit, plus the embedded first-party adapters (the
/// `bynk` surface and the `bynk.cloudflare` platform adapter), and call `f`
/// for each. Recovery parsing tolerates the in-progress edit at the cursor.
pub(crate) fn for_each_unit(
    doc_text: &str,
    src_root: Option<&Path>,
    mut f: impl FnMut(&SourceUnit),
) {
    let mut sources: Vec<String> = vec![
        BYNK_ADAPTER_SRC.to_string(),
        CLOUDFLARE_ADAPTER_SRC.to_string(),
        // The embedded stdlib commons (`bynk.list`/`bynk.map`/`bynk.string`) so
        // their free fns are enumerable for `uses`-imported completion (G5) and
        // signature help. Harmless to the other contexts — these units declare
        // only `fn`s (no types/capabilities), and they are `commons`, never a
        // `consumes` target.
        BYNK_LIST_SRC.to_string(),
        BYNK_MAP_SRC.to_string(),
        BYNK_STRING_SRC.to_string(),
        doc_text.to_string(),
    ];
    if let Some(root) = src_root {
        for path in walk_bynk_files(root) {
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

/// Consumable unit names: contexts and adapters (plus `bynk`), deduplicated.
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
        if let bynk_syntax::ast::CommonsItem::Capability(c) = item {
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

static EMPTY_CONSUMES: Vec<bynk_syntax::ast::ConsumesDecl> = Vec::new();

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
    fn consumes_target_suggests_units_including_bynk() {
        // An adapter in the open doc plus the always-available `bynk` surface.
        let doc = "adapter tokens {\n  binding \"./b.ts\"\n  capability Jwt { fn f() -> Effect[Int] }\n  provides Jwt = X\n}\n";
        let got = labels("  consumes ", doc);
        assert!(got.contains(&"bynk".to_string()), "{got:?}");
        assert!(got.contains(&"tokens".to_string()), "{got:?}");
    }

    #[test]
    fn consumes_brace_suggests_that_units_capabilities() {
        let got = labels("  consumes bynk { ", "context a.b\n");
        // The embedded `bynk` surface exports these.
        assert!(got.contains(&"Clock".to_string()), "{got:?}");
        assert!(got.contains(&"Random".to_string()), "{got:?}");
        assert!(got.contains(&"Logger".to_string()), "{got:?}");
    }

    #[test]
    fn given_suggests_local_and_flattened_capabilities() {
        let doc = "context a.b\n\
                   consumes bynk { Clock }\n\
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
    fn expression_position_offers_constructors_and_types() {
        // ADR 0093 D3/D5: a value position (after `=`) yields every constructor
        // keyword and in-scope type names — the entry to a static call or a
        // record construction. (Locals/params are appended handler-side, not by
        // `complete()`.) Registry-driven over CONSTRUCTORS.
        let doc = "commons m {\n  type Order = { id: Int }\n}\n";
        let items = complete("  let x = ", doc, None);
        for &c in CONSTRUCTORS {
            assert!(
                find(&items, c, CompletionKind::Constructor).is_some(),
                "constructor {c}: {:?}",
                items.iter().map(|i| &i.label).collect::<Vec<_>>()
            );
        }
        assert!(
            find(&items, "Int", CompletionKind::Type).is_some(),
            "builtin type"
        );
        assert!(
            find(&items, "Order", CompletionKind::Type).is_some(),
            "project type"
        );
    }

    #[test]
    fn value_receiver_and_decimal_are_not_expression_positions() {
        // A trailing `x.`/`1.` is a member/decimal context, not an expression
        // start — `complete()` yields nothing (the value-receiver path is
        // handler-side; see `record_value_and_decimal_receivers_yield_nothing`).
        assert!(complete("  let p = q.", "context a.b\n", None).is_empty());
        assert!(complete("  let n = 1.", "context a.b\n", None).is_empty());
    }

    /// Free `fn` names declared in a unit source (registry-driven test helper).
    fn free_fn_names(src: &str) -> Vec<String> {
        let tokens = lexer::tokenize(src).unwrap();
        let (unit, _) = parser::parse_unit_with_recovery(&tokens, src);
        let unit = unit.unwrap();
        let (items, _) = unit_items_and_uses(&unit);
        items
            .iter()
            .filter_map(|it| match it {
                CommonsItem::Fn(f) => match &f.name {
                    FnName::Free(id) => Some(id.name.clone()),
                    FnName::Method { .. } => None,
                },
                _ => None,
            })
            .collect()
    }

    #[test]
    fn free_functions_offered_for_own_unit_and_used_modules() {
        // ADR 0093 D3/G5: expression position offers the current unit's own
        // free `fn`s and the combinators of every `uses`-imported module.
        let doc = "commons app {\n  uses bynk.list\n  fn helper(x: Int) -> Int { x }\n}\n";
        let items = complete("  let y = ", doc, None);
        // The current unit's own function.
        assert!(
            find(&items, "helper", CompletionKind::Function).is_some(),
            "own fn: {:?}",
            items.iter().map(|i| &i.label).collect::<Vec<_>>()
        );
        // Every combinator of the imported `bynk.list` — registry-driven over the
        // embedded source, so a new stdlib combinator must surface or this fails.
        for name in free_fn_names(BYNK_LIST_SRC) {
            assert!(
                find(&items, &name, CompletionKind::Function).is_some(),
                "bynk.list.{name}: {:?}",
                items.iter().map(|i| &i.label).collect::<Vec<_>>()
            );
        }
        // A module that is not imported does not leak its fns.
        assert!(
            find(&items, "values", CompletionKind::Function).is_none(),
            "bynk.map.values leaked without `uses bynk.map`"
        );
    }

    #[test]
    fn free_functions_require_a_uses_import() {
        // Own fns are always in scope; stdlib combinators only with their `uses`.
        let doc = "commons app {\n  fn helper(x: Int) -> Int { x }\n}\n";
        let items = complete("  let y = ", doc, None);
        assert!(find(&items, "helper", CompletionKind::Function).is_some());
        for name in ["map", "filter", "reverse"] {
            assert!(
                find(&items, name, CompletionKind::Function).is_none(),
                "bynk.list.{name} offered without `uses bynk.list`"
            );
        }
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
        // Built-ins (with registry docs), the `bynk`-surface transparent types,
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
        // A bare `[` opening a list literal is an expression, not type args…
        assert!(!is_type_position("  let xs = ["));
        // …so it is an expression position: a list element is a value, and the
        // constructor keywords are offered there (ADR 0093 D3) — not a
        // type-argument completion.
        let items = complete("  let xs = [", "context a.b\n", None);
        assert!(
            find(&items, "Some", CompletionKind::Constructor).is_some(),
            "{:?}",
            items.iter().map(|c| &c.label).collect::<Vec<_>>()
        );
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
        let doc = "context a.b\n  capability Timer { fn now() -> Effect[Int]\n  fn at(t: Int) -> Effect[()] }\n";
        let items = complete("    Timer.", doc, None);
        let now = find(&items, "now", CompletionKind::Member).expect("`now` op offered");
        // Slice 5 detail polish: a typed signature (params + return), not bare
        // param names.
        assert_eq!(
            now.detail.as_deref(),
            Some("now() -> Effect[Int] — operation of `Timer`")
        );
        let at = find(&items, "at", CompletionKind::Member).expect("`at` op offered");
        assert_eq!(
            at.detail.as_deref(),
            Some("at(t: Int) -> Effect[()] — operation of `Timer`")
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
        let http: Vec<&str> = bynk_syntax::ast::HTTP_VARIANTS
            .iter()
            .map(|v| v.name)
            .collect();
        let queue: Vec<&str> = bynk_syntax::ast::QUEUE_VARIANTS
            .iter()
            .map(|v| v.name)
            .collect();
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
        use bynk_syntax::ast::BaseType;
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
        use bynk_check::checker::NamedKind;
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

    // -- v0.124 (slice 3): the non-keyword completion contexts --

    #[test]
    fn record_construction_offers_field_names() {
        let doc = "commons m {\n  type Order = { id: Int, total: Int }\n}\n";
        let got = labels("  let o = Order { ", doc);
        assert!(got.contains(&"id".to_string()), "{got:?}");
        assert!(got.contains(&"total".to_string()), "{got:?}");
        // After a comma, still field-name position.
        let got2 = labels("  let o = Order { id: 1, ", doc);
        assert!(got2.contains(&"total".to_string()), "{got2:?}");
        // After a `:`, it is a field *type* position, not a field name.
        assert!(record_construction_receiver("  let o = Order { id: ").is_none());
        // A lowercase brace context (a block) is not a construction.
        assert!(record_construction_receiver("  if x { ").is_none());
    }

    #[test]
    fn from_offers_protocols() {
        let got = labels("  service s from ", "context a.b\n");
        assert!(got.contains(&"http".to_string()), "{got:?}");
        assert!(got.contains(&"cron".to_string()) && got.contains(&"queue".to_string()));
    }

    #[test]
    fn on_offers_handler_kinds() {
        let got = labels("  on ", "context a.b\n");
        assert!(got.contains(&"call".to_string()), "{got:?}");
        assert!(got.contains(&"GET".to_string()) && got.contains(&"schedule".to_string()));
    }

    #[test]
    fn by_offers_project_actors() {
        let doc = "context a.b\n\nactor Caller { auth = Bearer }\n";
        let got = labels("    by ", doc);
        assert!(got.contains(&"Caller".to_string()), "{got:?}");
    }

    #[test]
    fn exports_offers_export_kinds() {
        let got = labels("  exports ", "adapter t {\n  binding \"./b.ts\"\n}\n");
        assert!(got.contains(&"capability".to_string()), "{got:?}");
        assert!(got.contains(&"transparent".to_string()));
    }

    #[test]
    fn provides_offers_in_scope_capabilities() {
        let doc = "context a.b\n\ncapability Store { fn get() -> Effect[Int] }\n";
        let got = labels("  provides ", doc);
        assert!(got.contains(&"Store".to_string()), "{got:?}");
    }

    #[test]
    fn clause_detectors_do_not_over_fire() {
        // `on` inside a larger word, and a field named `from`, must not trigger.
        assert!(!after_clause_keyword("  session ", "on"));
        assert!(!after_clause_keyword("  let from = ", "from"));
        // A standalone keyword does.
        assert!(after_clause_keyword("  service s from ", "from"));
        assert!(after_clause_keyword("    by ", "by"));
    }

    #[test]
    fn contract_clause_kind_detects_requires_and_ensures() {
        assert_eq!(contract_clause_kind("  requires positive: "), Some(false));
        assert_eq!(contract_clause_kind("  ensures never_neg: "), Some(true));
        // Not a contract clause.
        assert_eq!(contract_clause_kind("  id: Int"), None);
        assert_eq!(contract_clause_kind("  let x = 1"), None);
    }

    #[test]
    fn sum_type_variants_lists_variants() {
        let doc = "commons m {\n  type Status = enum { Pending, Shipped }\n}\n";
        let got: Vec<String> = sum_type_variants("Status", doc, None)
            .into_iter()
            .map(|c| c.label)
            .collect();
        assert!(got.contains(&"Pending".to_string()), "{got:?}");
        assert!(got.contains(&"Shipped".to_string()), "{got:?}");
    }
}
