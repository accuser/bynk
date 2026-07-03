//! Symbol lookups for hover and go-to-definition.
//!
//! Single-file lookups walk the parsed AST. Cross-file lookups (v1.1; LSP
//! spec §3.4 cross-file requirement) iterate the project's `.bynk` sources
//! to find a declaration in any unit the user might be referencing — used
//! when the open file lacks the symbol the user clicked on (typically
//! because the name was imported via `uses` or made available via
//! `consumes`).

use std::path::{Path, PathBuf};

use bynk_syntax::ast::*;
use bynk_syntax::lexer::tokenize;
use bynk_syntax::parser::parse_unit_with_recovery;
use bynk_syntax::span::Span;
use tower_lsp::lsp_types::Url;

/// Return the source span of the declaration named `name` in the given
/// source text. Returns `None` if no declaration matches.
pub fn find_declaration_span(source: &str, name: &str) -> Option<Span> {
    let tokens = tokenize(source).ok()?;
    let (unit, _errs) = parse_unit_with_recovery(&tokens, source);
    let unit = unit?;
    let items: &[CommonsItem] = match &unit {
        SourceUnit::Commons(c) => &c.items,
        SourceUnit::Context(c) => &c.items,
        SourceUnit::Adapter(a) => &a.items,
        SourceUnit::Suite(_) => &[],
    };
    for item in items {
        match item {
            CommonsItem::Type(t) if t.name.name == name => return Some(t.name.span),
            CommonsItem::Fn(f) if f.name.ident().name == name => return Some(f.name.ident().span),
            CommonsItem::Capability(c) if c.name.name == name => return Some(c.name.span),
            CommonsItem::Service(s) if s.name.name == name => return Some(s.name.span),
            CommonsItem::Agent(a) if a.name.name == name => return Some(a.name.span),
            CommonsItem::Provider(p) if p.provider_name.name == name => {
                return Some(p.provider_name.span);
            }
            _ => {}
        }
    }
    None
}

/// Build a Markdown summary of a named declaration suitable for an LSP
/// hover response. Returns `None` if no declaration matches.
pub fn describe_symbol(source: &str, name: &str) -> Option<String> {
    let tokens = tokenize(source).ok()?;
    let (unit, _errs) = parse_unit_with_recovery(&tokens, source);
    let unit = unit?;
    let items: &[CommonsItem] = match &unit {
        SourceUnit::Commons(c) => &c.items,
        SourceUnit::Context(c) => &c.items,
        SourceUnit::Adapter(a) => &a.items,
        SourceUnit::Suite(_) => &[],
    };
    for item in items {
        if let Some(summary) = describe_item(item, name) {
            return Some(summary);
        }
    }
    None
}

/// v0.121 (ADR 0156): the reserved-keyword doc for the token at `offset` in
/// `source`, if the cursor sits on one — matched by source text against
/// `bynk_syntax::keywords::KEYWORDS`, independent of the token's `TokenKind`
/// (unlike the identifier-only lexical hover fallback above). This is hover's
/// floor for the mechanical coverage test: every lowercase-initial keyword
/// gets at least this, even where `describe_symbol` has no richer path for it
/// (e.g. the testing-track clause keywords — `requires`/`ensures`/`suite`/…).
pub fn describe_keyword_at(source: &str, offset: usize) -> Option<&'static str> {
    let tokens = tokenize(source).ok()?;
    let word = tokens
        .iter()
        .find(|t| t.span.start <= offset && offset < t.span.end)
        .map(|t| &source[t.span.start..t.span.end])?;
    bynk_syntax::keywords::KEYWORDS
        .iter()
        .find(|k| k.word == word)
        .map(|k| k.meaning)
}

/// v0.122 (editor-currency slice 1): a hover summary for `self` under the
/// cursor — `self: <Type>`. `self` is a reserved keyword (never an `Ident`, so
/// it does not flow through `locals_nav`), but a `self` *use* is a typed
/// expression, so its type is in `expr_types` at the token's span. For a method
/// the type is the receiver's name; for an agent handler the checker gives
/// `self` a synthetic record type `__<Agent>Self` (to resolve `self.<key>`),
/// which is un-synthesised here to `<Agent>`. `None` when the cursor is not on
/// the `self` keyword or its type is unknown (a broken buffer — `expr_types` is
/// clean-file-only, so this degrades to the keyword doc, never a wrong type).
pub fn describe_self_at(
    text: &str,
    offset: usize,
    expr_types: &[(Span, bynk_check::checker::Ty)],
) -> Option<String> {
    let tokens = tokenize(text).ok()?;
    let on_self = tokens.iter().any(|t| {
        t.span.start <= offset && offset < t.span.end && &text[t.span.start..t.span.end] == "self"
    });
    if !on_self {
        return None;
    }
    let ty = bynk_check::expr_types::type_at_offset(expr_types, offset)?;
    let display = ty.display();
    let name = display
        .strip_prefix("__")
        .and_then(|s| s.strip_suffix("Self"))
        .unwrap_or(&display);
    Some(format!("```bynk\nself: {name}\n```"))
}

/// v0.123 (editor-currency slice 2, DECISION B): if the identifier at
/// `ident_span` is the member of an `Upper.member` name-receiver access
/// (`Clock.now`, `Email.of`), return the full `Recv.member` callee for
/// [`crate::signature_help::resolve_label`] to resolve to its signature — the
/// same resolution completion and signature help perform, no new index.
/// `None` for a bare identifier or a lowercase (value-receiver) method, which
/// `resolve_label` does not handle.
pub(crate) fn qualified_callee_at(text: &str, ident_span: Span) -> Option<String> {
    let before = text.get(..ident_span.start)?.strip_suffix('.')?;
    let recv_start = before
        .rfind(|c: char| !(c.is_alphanumeric() || c == '_'))
        .map_or(0, |i| i + 1);
    let recv = &before[recv_start..];
    if !recv.chars().next()?.is_uppercase() {
        return None;
    }
    let member = text.get(ident_span.start..ident_span.end)?;
    Some(format!("{recv}.{member}"))
}

/// Describe a symbol declared in the embedded first-party sources — the `bynk`
/// and `bynk.cloudflare` adapters and the `bynk.list`/`bynk.map`/`bynk.string`
/// stdlib. Hover and completion-doc resolution otherwise walk only the project's
/// files (`walk_bynk_files`), so stdlib/surface symbols had no surfaced signature
/// or doc; this is the fallback after the project scan. Any `---` doc block on a
/// first-party declaration rides along (via `describe_fn`/`describe_type`/…),
/// once the sources carry one.
pub(crate) fn describe_firstparty_symbol(name: &str) -> Option<String> {
    const SOURCES: &[&str] = &[
        bynk_check::firstparty::BYNK_ADAPTER_SRC,
        bynk_check::firstparty::CLOUDFLARE_ADAPTER_SRC,
        bynk_check::firstparty::BYNK_LIST_SRC,
        bynk_check::firstparty::BYNK_MAP_SRC,
        bynk_check::firstparty::BYNK_STRING_SRC,
    ];
    SOURCES.iter().find_map(|src| describe_symbol(src, name))
}

/// Slice 6b: the `(unit name, name span)` of every `uses`/`consumes` target in
/// the source — the clickable ranges for document links. The link's target file
/// is resolved by the handler through the unit→source map (ADR 0095); this only
/// finds the spans, so it works on the live buffer regardless of the map.
pub(crate) fn unit_reference_spans(source: &str) -> Vec<(String, Span)> {
    let Ok(tokens) = tokenize(source) else {
        return Vec::new();
    };
    let (Some(unit), _) = parse_unit_with_recovery(&tokens, source) else {
        return Vec::new();
    };
    let (uses, consumes): (&[UsesDecl], &[ConsumesDecl]) = match &unit {
        SourceUnit::Commons(c) => (&c.uses, &[]),
        SourceUnit::Context(c) => (&c.uses, &c.consumes),
        SourceUnit::Adapter(a) => (&a.uses, &a.consumes),
        SourceUnit::Suite(_) => (&[], &[]),
    };
    let mut out: Vec<(String, Span)> = Vec::new();
    for u in uses {
        out.push((u.target.joined(), u.target.span));
    }
    for c in consumes {
        out.push((c.target.joined(), c.target.span));
    }
    out
}

fn describe_item(item: &CommonsItem, name: &str) -> Option<String> {
    match item {
        CommonsItem::Type(t) if t.name.name == name => Some(describe_type(t)),
        CommonsItem::Fn(f) if f.name.ident().name == name => Some(describe_fn(f)),
        CommonsItem::Capability(c) if c.name.name == name => Some(describe_capability(c)),
        CommonsItem::Service(s) if s.name.name == name => Some(describe_service(s)),
        CommonsItem::Agent(a) if a.name.name == name => Some(describe_agent(a)),
        CommonsItem::Provider(p) if p.provider_name.name == name => Some(describe_provider(p)),
        _ => None,
    }
}

fn describe_type(t: &TypeDecl) -> String {
    let mut out = String::new();
    out.push_str("```bynk\n");
    out.push_str(&format!("type {} = ", t.name.name));
    match &t.body {
        // v0.123 (slice 2): render the refined/opaque `where` predicate (was
        // collapsed to the bare base) via the formatter's own renderer.
        TypeBody::Refined {
            base, refinement, ..
        } => {
            out.push_str(base.name());
            if let Some(r) = refinement {
                out.push_str(&format!(" where {}", bynk_fmt::refinement_to_string(r)));
            }
        }
        TypeBody::Opaque {
            base, refinement, ..
        } => {
            out.push_str(&format!("opaque {}", base.name()));
            if let Some(r) = refinement {
                out.push_str(&format!(" where {}", bynk_fmt::refinement_to_string(r)));
            }
        }
        // Record fields, one per line (was collapsed to `record`).
        TypeBody::Record(r) => {
            if r.fields.is_empty() {
                out.push_str("{}");
            } else {
                out.push_str("{\n");
                for f in &r.fields {
                    out.push_str(&format!("\t{}: {}", f.name.name, type_ref_str(&f.type_ref)));
                    if let Some(r) = &f.refinement {
                        out.push_str(&format!(" where {}", bynk_fmt::refinement_to_string(r)));
                    }
                    out.push_str(",\n");
                }
                out.push('}');
            }
        }
        // Sum variants, with payloads (was collapsed to `sum`).
        TypeBody::Sum(s) => {
            out.push_str("enum {\n");
            for v in &s.variants {
                out.push_str(&format!("\t{}", v.name.name));
                if !v.payload.is_empty() {
                    let parts: Vec<String> = v
                        .payload
                        .iter()
                        .map(|p| format!("{}: {}", p.name.name, type_ref_str(&p.type_ref)))
                        .collect();
                    out.push_str(&format!("({})", parts.join(", ")));
                }
                out.push_str(",\n");
            }
            out.push('}');
        }
    }
    out.push_str("\n```\n");
    if let Some(doc) = &t.documentation {
        out.push('\n');
        out.push_str(doc);
        out.push('\n');
    }
    out
}

fn describe_fn(f: &FnDecl) -> String {
    let mut out = String::new();
    out.push_str("```bynk\n");
    out.push_str("fn ");
    out.push_str(&f.name.display());
    out.push('(');
    let mut parts: Vec<String> = Vec::new();
    if f.has_self {
        parts.push("self".into());
    }
    for p in &f.params {
        parts.push(format!("{}: {}", p.name.name, type_ref_str(&p.type_ref)));
    }
    out.push_str(&parts.join(", "));
    out.push_str(") -> ");
    out.push_str(&type_ref_str(&f.return_type));
    // v0.123 (slice 2): the contract clauses (v0.115), beneath the signature —
    // rendered through the formatter's own predicate renderer.
    for c in &f.requires {
        out.push_str(&format!(
            "\n\trequires {}: {}",
            c.name.name,
            bynk_fmt::expr_to_string(&c.predicate)
        ));
    }
    for c in &f.ensures {
        out.push_str(&format!(
            "\n\tensures {}: {}",
            c.name.name,
            bynk_fmt::expr_to_string(&c.predicate)
        ));
    }
    out.push_str("\n```\n");
    if let Some(doc) = &f.documentation {
        out.push('\n');
        out.push_str(doc);
        out.push('\n');
    }
    out
}

fn describe_capability(c: &CapabilityDecl) -> String {
    let mut out = String::new();
    out.push_str("```bynk\ncapability ");
    out.push_str(&c.name.name);
    out.push_str(" {\n");
    for op in &c.ops {
        out.push_str("\tfn ");
        out.push_str(&op.name.name);
        out.push('(');
        let parts: Vec<String> = op
            .params
            .iter()
            .map(|p| format!("{}: {}", p.name.name, type_ref_str(&p.type_ref)))
            .collect();
        out.push_str(&parts.join(", "));
        out.push_str(") -> ");
        out.push_str(&type_ref_str(&op.return_type));
        out.push('\n');
    }
    out.push_str("}\n```\n");
    if let Some(doc) = &c.documentation {
        out.push('\n');
        out.push_str(doc);
        out.push('\n');
    }
    out
}

/// v0.123 (slice 2): the `from <protocol>` header suffix for a service, or the
/// empty string for a plain `on call` service.
fn service_protocol_suffix(p: &ServiceProtocol) -> String {
    match p {
        ServiceProtocol::Call => String::new(),
        ServiceProtocol::Http => " from http".to_string(),
        ServiceProtocol::Cron => " from cron".to_string(),
        ServiceProtocol::Queue { name } => format!(" from queue(\"{name}\")"),
        ServiceProtocol::WebSocket { .. } => " from WebSocket".to_string(),
    }
}

/// v0.123 (slice 2): the `on …` line for a handler — its route/protocol shape.
fn handler_line(h: &Handler) -> String {
    match &h.kind {
        HandlerKind::Call => "on call".to_string(),
        HandlerKind::Http { method, path } => format!("on {}(\"{}\")", method.as_str(), path),
        HandlerKind::Cron { expr } => format!("on schedule(\"{expr}\")"),
        HandlerKind::Message => "on message".to_string(),
        HandlerKind::Open => "on open".to_string(),
        HandlerKind::Close => "on close".to_string(),
    }
}

fn describe_service(s: &ServiceDecl) -> String {
    // v0.123 (slice 2): the protocol header and a line per route (was a bare
    // handler count).
    let mut out = format!(
        "```bynk\nservice {}{} {{\n",
        s.name.name,
        service_protocol_suffix(&s.protocol)
    );
    for h in &s.handlers {
        out.push_str(&format!("\t{}\n", handler_line(h)));
    }
    out.push_str("}\n```\n");
    if let Some(doc) = &s.documentation {
        out.push('\n');
        out.push_str(doc);
        out.push('\n');
    }
    out
}

/// v0.123 (slice 2): a store field's kind — `Cell[Int]`, `Map[K, V]`, or a bare
/// head with no type args.
fn store_kind_str(k: &StoreKind) -> String {
    if k.args.is_empty() {
        k.head.name.clone()
    } else {
        let args: Vec<String> = k.args.iter().map(type_ref_str).collect();
        format!("{}[{}]", k.head.name, args.join(", "))
    }
}

fn describe_agent(a: &AgentDecl) -> String {
    // v0.123 (slice 2): the store fields plus the `invariant`/`transition` step
    // invariants (v0.116), was a bare store-field count.
    let mut out = format!(
        "```bynk\nagent {} {{\n\tkey {}: {}\n",
        a.name.name,
        a.key_name.name,
        type_ref_str(&a.key_type),
    );
    for f in &a.store_fields {
        out.push_str(&format!(
            "\tstore {}: {}\n",
            f.name.name,
            store_kind_str(&f.kind)
        ));
    }
    for inv in &a.invariants {
        out.push_str(&format!(
            "\tinvariant {}: {}\n",
            inv.name.name,
            bynk_fmt::expr_to_string(&inv.predicate)
        ));
    }
    for tr in &a.transitions {
        out.push_str(&format!(
            "\ttransition {}: {}\n",
            tr.name.name,
            bynk_fmt::expr_to_string(&tr.predicate)
        ));
    }
    out.push_str("}\n```\n");
    if let Some(doc) = &a.documentation {
        out.push('\n');
        out.push_str(doc);
        out.push('\n');
    }
    out
}

fn describe_provider(p: &ProviderDecl) -> String {
    let mut out = format!(
        "```bynk\nprovides {} = {}\n```\n",
        p.capability.name, p.provider_name.name
    );
    if let Some(doc) = &p.documentation {
        out.push('\n');
        out.push_str(doc);
        out.push('\n');
    }
    out
}

/// A cross-file declaration lookup result: the URI of the file containing
/// the declaration, the declaration's source span, and the full source
/// text of that file (returned because callers need it to convert the
/// span to an LSP range and to build hover content).
pub struct CrossFileSymbol {
    pub uri: Url,
    pub span: Span,
    pub source: String,
}

/// Find `name`'s declaration in any project file other than `current_uri`.
/// Walks `src_root` recursively, parses each `.bynk` file with recovery,
/// and returns the first hit. Returns `None` if the name is not found
/// anywhere in the project.
///
/// Caller is responsible for trying the open file's local symbol table
/// first; this function intentionally skips `current_uri` so the local
/// path remains the fast path.
pub fn find_declaration_cross_file(
    src_root: &Path,
    current_uri: &Url,
    name: &str,
) -> Option<CrossFileSymbol> {
    for path in walk_bynk_files(src_root) {
        let Ok(uri) = Url::from_file_path(&path) else {
            continue;
        };
        if &uri == current_uri {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Some(span) = find_declaration_span(&source, name) {
            return Some(CrossFileSymbol { uri, span, source });
        }
    }
    None
}

/// Markdown hover content for `name` from any project file other than
/// `current_uri`, plus the URI of the file that contributed it. Returns
/// `None` if the name is not declared anywhere in the project.
pub fn describe_symbol_cross_file(
    src_root: &Path,
    current_uri: &Url,
    name: &str,
) -> Option<(Url, String)> {
    for path in walk_bynk_files(src_root) {
        let Ok(uri) = Url::from_file_path(&path) else {
            continue;
        };
        if &uri == current_uri {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Some(desc) = describe_symbol(&source, name) {
            return Some((uri, desc));
        }
    }
    None
}

/// Recursively collect every `.bynk` file under `root`. Returns an empty
/// vector if the root is missing or unreadable.
pub(crate) fn walk_bynk_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|e| e.to_str()) == Some("bynk") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

pub(crate) fn type_ref_str(t: &TypeRef) -> String {
    match t {
        // v0.20a: function types render in Bynk surface syntax.
        TypeRef::Fn(params, ret, _) => {
            let lhs = match params.len() {
                0 => "()".to_string(),
                1 if !matches!(params[0], TypeRef::Fn(..)) => type_ref_str(&params[0]),
                _ => format!(
                    "({})",
                    params
                        .iter()
                        .map(type_ref_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            };
            format!("{lhs} -> {}", type_ref_str(ret))
        }
        TypeRef::Base(b, _) => b.name().to_string(),
        TypeRef::Named(id) => id.name.clone(),
        TypeRef::Result(a, b, _) => format!("Result[{}, {}]", type_ref_str(a), type_ref_str(b)),
        TypeRef::Option(t, _) => format!("Option[{}]", type_ref_str(t)),
        TypeRef::Effect(t, _) => format!("Effect[{}]", type_ref_str(t)),
        TypeRef::HttpResult(t, _) => format!("HttpResult[{}]", type_ref_str(t)),
        TypeRef::QueueResult(_) => "QueueResult".to_string(),
        // v0.20b: the built-in collection types.
        TypeRef::List(t, _) => format!("List[{}]", type_ref_str(t)),
        TypeRef::Query(t, _) => format!("Query[{}]", type_ref_str(t)),
        TypeRef::Stream(t, _) => format!("Stream[{}]", type_ref_str(t)),
        TypeRef::Connection(t, _) => format!("Connection[{}]", type_ref_str(t)),
        TypeRef::History(t, _) => format!("History[{}]", type_ref_str(t)),
        TypeRef::Map(k, v, _) => format!("Map[{}, {}]", type_ref_str(k), type_ref_str(v)),
        TypeRef::ValidationError(_) => "ValidationError".to_string(),
        TypeRef::JsonError(_) => "JsonError".to_string(),
        TypeRef::Unit(_) => "()".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Build a temp directory unique to the test name, populate it with
    /// `(relative_path, contents)` files, and return the root path. The
    /// directory is left behind on the filesystem; callers can clean up
    /// if they care.
    fn setup_project(test_name: &str, files: &[(&str, &str)]) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "bynk-lsp-test-{}-{}",
            test_name,
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create test root");
        for (rel, contents) in files {
            let p = root.join(rel);
            if let Some(parent) = p.parent() {
                fs::create_dir_all(parent).expect("create parent");
            }
            fs::write(&p, contents).expect("write file");
        }
        root
    }

    #[test]
    fn cross_file_definition_resolves_into_sibling_file() {
        let root = setup_project(
            "cross_file_definition",
            &[
                (
                    "a.bynk",
                    "commons demo.a\n\ntype Foo = Int where Positive\n",
                ),
                (
                    "b.bynk",
                    "commons demo.b\n\nuses demo.a\n\ntype Bar = Int where NonNegative\n",
                ),
            ],
        );
        let current = Url::from_file_path(root.join("b.bynk")).unwrap();
        let found = find_declaration_cross_file(&root, &current, "Foo")
            .expect("Foo should resolve into a.bynk");
        let expected = Url::from_file_path(root.join("a.bynk")).unwrap();
        assert_eq!(found.uri, expected);
        assert!(
            found.source.contains("type Foo = Int where Positive"),
            "source returned does not contain Foo declaration"
        );
    }

    #[test]
    fn cross_file_definition_skips_current_file() {
        let root = setup_project(
            "cross_file_skip_current",
            &[(
                "only.bynk",
                "commons demo.only\n\ntype Foo = Int where Positive\n",
            )],
        );
        let current = Url::from_file_path(root.join("only.bynk")).unwrap();
        // The only file containing Foo is current; cross-file must skip it.
        assert!(find_declaration_cross_file(&root, &current, "Foo").is_none());
    }

    #[test]
    fn cross_file_hover_returns_markdown_summary() {
        let root = setup_project(
            "cross_file_hover",
            &[
                (
                    "money.bynk",
                    "commons demo.money\n\n\
                     ---\n\
                     Amount in minor units of currency.\n\
                     ---\n\
                     type Money = Int where NonNegative\n",
                ),
                (
                    "orders.bynk",
                    "commons demo.orders\n\nuses demo.money\n\ntype OrderId = Int where Positive\n",
                ),
            ],
        );
        let current = Url::from_file_path(root.join("orders.bynk")).unwrap();
        let (other_uri, desc) = describe_symbol_cross_file(&root, &current, "Money")
            .expect("Money should produce hover content");
        assert_eq!(
            other_uri,
            Url::from_file_path(root.join("money.bynk")).unwrap()
        );
        assert!(desc.contains("type Money"));
        assert!(
            desc.contains("Amount in minor units"),
            "hover should include the doc block"
        );
    }

    #[test]
    fn cross_file_returns_none_for_unknown_name() {
        let root = setup_project(
            "cross_file_none",
            &[(
                "a.bynk",
                "commons demo.a\n\ntype Foo = Int where Positive\n",
            )],
        );
        let current = Url::from_file_path(root.join("a.bynk")).unwrap();
        assert!(find_declaration_cross_file(&root, &current, "DoesNotExist").is_none());
        assert!(describe_symbol_cross_file(&root, &current, "DoesNotExist").is_none());
    }

    #[test]
    fn first_party_symbols_describe_their_signature_and_doc() {
        // Slice 9: stdlib/surface symbols live in the embedded sources, not the
        // project — the hover/completion-doc fallback finds them there, signature
        // and `---` doc block alike.
        let reverse = describe_firstparty_symbol("reverse").expect("`bynk.list.reverse` described");
        assert!(
            reverse.contains("reverse") && reverse.contains("List"),
            "{reverse}"
        );
        assert!(
            reverse.contains("reverse order"),
            "doc block surfaced: {reverse}"
        );
        // The `bynk` adapter surface too (a capability, exercising the adapter path).
        let clock = describe_firstparty_symbol("Clock").expect("`bynk`-surface `Clock`");
        assert!(
            clock.contains("wall-clock"),
            "capability doc surfaced: {clock}"
        );
        // A name in no first-party source yields nothing (the fallback no-ops).
        assert!(describe_firstparty_symbol("DoesNotExist").is_none());
    }

    #[test]
    fn unit_reference_spans_finds_uses_and_consumes_targets() {
        // Slice 6b: the clickable ranges for document links — `uses`/`consumes`
        // unit names, with spans covering the name (resolution is the handler's).
        let src = "context app.main\n  uses billing.charge\n  consumes platform.time\n";
        let spans = unit_reference_spans(src);
        let names: Vec<&str> = spans.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"billing.charge"), "{names:?}");
        assert!(names.contains(&"platform.time"), "{names:?}");
        // The span covers exactly the unit name (so the link underlines it).
        let (_, span) = spans.iter().find(|(n, _)| n == "billing.charge").unwrap();
        assert_eq!(&src[span.start..span.end], "billing.charge");
    }

    // v0.123 (slice 2): hover renders the real shape of each declaration —
    // record fields, sum variants, the refined `where`, the opaque base.
    #[test]
    fn describe_type_renders_fields_variants_and_refinements() {
        let record = describe_symbol(
            "commons demo.m\n\ntype Order = {\n  id: OrderId,\n  total: Money,\n}\n",
            "Order",
        )
        .unwrap();
        assert!(record.contains("type Order = {"), "{record}");
        assert!(record.contains("id: OrderId"), "{record}");
        assert!(record.contains("total: Money"), "{record}");

        let sum = describe_symbol(
            "commons demo.m\n\ntype Status = enum { Pending, Shipped }\n",
            "Status",
        )
        .unwrap();
        assert!(sum.contains("enum {"), "{sum}");
        assert!(sum.contains("Pending") && sum.contains("Shipped"), "{sum}");

        let refined = describe_symbol(
            "commons demo.m\n\ntype Email = String where NonEmpty\n",
            "Email",
        )
        .unwrap();
        assert!(
            refined.contains("type Email = String where NonEmpty"),
            "{refined}"
        );

        let opaque =
            describe_symbol("commons demo.m\n\ntype Token = opaque String\n", "Token").unwrap();
        assert!(opaque.contains("type Token = opaque String"), "{opaque}");
    }

    // v0.123 (slice 2): a function's `requires`/`ensures` contracts render
    // beneath its signature.
    #[test]
    fn describe_fn_renders_contracts() {
        let src = "commons demo.m\n\nfn discount(p: Int, pct: Int) -> Int\n  requires p_nonneg: p >= 0\n  ensures never_negative: result >= 0\n{\n  p\n}\n";
        let out = describe_symbol(src, "discount").unwrap();
        assert!(
            out.contains("fn discount(p: Int, pct: Int) -> Int"),
            "{out}"
        );
        assert!(out.contains("requires p_nonneg: p >= 0"), "{out}");
        assert!(out.contains("ensures never_negative: result >= 0"), "{out}");
    }

    // v0.123 (slice 2): a service renders its protocol header and route lines.
    #[test]
    fn describe_service_renders_routes() {
        let src = "context demo.app\n\nservice greeter {\n  on call(name: String) -> Effect[String] {\n    Effect.pure(name)\n  }\n}\n";
        let out = describe_symbol(src, "greeter").unwrap();
        assert!(out.contains("service greeter {"), "{out}");
        assert!(out.contains("on call"), "{out}");
    }

    // v0.123 (slice 2): an agent renders its store fields and the
    // `invariant`/`transition` step invariants.
    #[test]
    fn describe_agent_renders_store_and_invariants() {
        let src = "context demo.app\n\nagent Counter {\n  key id: String\n  store count: Cell[Int] = 0\n  invariant non_negative: count >= 0\n  transition monotonic: new.count >= old.count\n  on call bump() -> Effect[Result[(), String]] {\n    Ok(())\n  }\n}\n";
        let out = describe_symbol(src, "Counter").unwrap();
        assert!(out.contains("agent Counter {"), "{out}");
        assert!(out.contains("key id: String"), "{out}");
        assert!(out.contains("store count: Cell[Int]"), "{out}");
        assert!(out.contains("invariant non_negative: count >= 0"), "{out}");
        assert!(
            out.contains("transition monotonic: new.count >= old.count"),
            "{out}"
        );
    }

    // v0.123 (slice 2, DECISION B): the `Recv.member` detection that feeds
    // capability-op call-site hover through `resolve_label`.
    #[test]
    fn qualified_callee_detects_upper_receiver_only() {
        let text = "  let t = Clock.now()";
        let now = text.find("now").unwrap();
        assert_eq!(
            qualified_callee_at(
                text,
                Span {
                    start: now,
                    end: now + 3
                }
            )
            .as_deref(),
            Some("Clock.now")
        );
        // A lowercase (value) receiver is not our case — resolve_label can't
        // resolve it anyway.
        let text2 = "  xs.fold(0)";
        let fold = text2.find("fold").unwrap();
        assert!(
            qualified_callee_at(
                text2,
                Span {
                    start: fold,
                    end: fold + 4
                }
            )
            .is_none()
        );
        // A bare identifier (no receiver) → None.
        let text3 = "  total";
        assert!(qualified_callee_at(text3, Span { start: 2, end: 7 }).is_none());
    }

    // v0.122 (slice 1): `self` hover renders its receiver/agent type, reading
    // the type from `expr_types` and un-synthesising the agent-self record.
    #[test]
    fn describe_self_renders_receiver_and_unwraps_agent() {
        use bynk_check::checker::{NamedKind, Ty};
        let text = "self";
        let span = Span { start: 0, end: 4 };
        // A method receiver — a plain named type renders verbatim.
        let account = vec![(
            span,
            Ty::Named {
                name: "Account".into(),
                kind: NamedKind::Record,
            },
        )];
        assert_eq!(
            describe_self_at(text, 0, &account).as_deref(),
            Some("```bynk\nself: Account\n```")
        );
        // An agent handler — the synthetic `__CounterSelf` record un-synthesises
        // to the agent name.
        let agent = vec![(
            span,
            Ty::Named {
                name: "__CounterSelf".into(),
                kind: NamedKind::Record,
            },
        )];
        assert_eq!(
            describe_self_at(text, 0, &agent).as_deref(),
            Some("```bynk\nself: Counter\n```")
        );
        // Not on the `self` keyword — a different token yields nothing, even
        // when a type sits at the offset.
        let other = "total";
        assert!(
            describe_self_at(
                other,
                0,
                &[(
                    Span { start: 0, end: 5 },
                    Ty::Named {
                        name: "Int".into(),
                        kind: NamedKind::Record,
                    },
                )]
            )
            .is_none()
        );
    }
}
