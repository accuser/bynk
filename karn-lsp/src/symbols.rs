//! Symbol lookups for hover and go-to-definition.
//!
//! Single-file lookups walk the parsed AST. Cross-file lookups (v1.1; LSP
//! spec §3.4 cross-file requirement) iterate the project's `.karn` sources
//! to find a declaration in any unit the user might be referencing — used
//! when the open file lacks the symbol the user clicked on (typically
//! because the name was imported via `uses` or made available via
//! `consumes`).

use std::path::{Path, PathBuf};

use karnc::ast::*;
use karnc::lexer::tokenize;
use karnc::parser::parse_unit_with_recovery;
use karnc::span::Span;
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
        SourceUnit::Test(_) | SourceUnit::Integration(_) => &[],
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
        SourceUnit::Test(_) | SourceUnit::Integration(_) => &[],
    };
    for item in items {
        if let Some(summary) = describe_item(item, name) {
            return Some(summary);
        }
    }
    None
}

/// Describe a symbol declared in the embedded first-party sources — the `karn`
/// and `karn.cloudflare` adapters and the `karn.list`/`karn.map`/`karn.string`
/// stdlib. Hover and completion-doc resolution otherwise walk only the project's
/// files (`walk_karn_files`), so stdlib/surface symbols had no surfaced signature
/// or doc; this is the fallback after the project scan. Any `---` doc block on a
/// first-party declaration rides along (via `describe_fn`/`describe_type`/…),
/// once the sources carry one.
pub(crate) fn describe_firstparty_symbol(name: &str) -> Option<String> {
    const SOURCES: &[&str] = &[
        karnc::firstparty::KARN_ADAPTER_SRC,
        karnc::firstparty::CLOUDFLARE_ADAPTER_SRC,
        karnc::firstparty::KARN_LIST_SRC,
        karnc::firstparty::KARN_MAP_SRC,
        karnc::firstparty::KARN_STRING_SRC,
    ];
    SOURCES.iter().find_map(|src| describe_symbol(src, name))
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
    out.push_str("```karn\n");
    let body = match &t.body {
        TypeBody::Refined { base, .. } => format!("type {} = {}", t.name.name, base.name()),
        TypeBody::Opaque { base, .. } => format!("type {} = opaque {}", t.name.name, base.name()),
        TypeBody::Record(_) => format!("type {} = record", t.name.name),
        TypeBody::Sum(_) => format!("type {} = sum", t.name.name),
    };
    out.push_str(&body);
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
    out.push_str("```karn\n");
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
    out.push_str("```karn\ncapability ");
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

fn describe_service(s: &ServiceDecl) -> String {
    let mut out = format!("```karn\nservice {}\n```\n", s.name.name);
    if let Some(doc) = &s.documentation {
        out.push('\n');
        out.push_str(doc);
        out.push('\n');
    }
    out.push_str(&format!("\n{} handler(s).", s.handlers.len()));
    out
}

fn describe_agent(a: &AgentDecl) -> String {
    let mut out = format!(
        "```karn\nagent {} {{\n\tkey {}: {}\n\tstate {{ {} field(s) }}\n}}\n```\n",
        a.name.name,
        a.key_name.name,
        type_ref_str(&a.key_type),
        a.state_fields.len(),
    );
    if let Some(doc) = &a.documentation {
        out.push('\n');
        out.push_str(doc);
        out.push('\n');
    }
    out
}

fn describe_provider(p: &ProviderDecl) -> String {
    let mut out = format!(
        "```karn\nprovides {} = {}\n```\n",
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
/// Walks `src_root` recursively, parses each `.karn` file with recovery,
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
    for path in walk_karn_files(src_root) {
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
    for path in walk_karn_files(src_root) {
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

/// Recursively collect every `.karn` file under `root`. Returns an empty
/// vector if the root is missing or unreadable.
pub(crate) fn walk_karn_files(root: &Path) -> Vec<PathBuf> {
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
            } else if p.extension().and_then(|e| e.to_str()) == Some("karn") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

pub(crate) fn type_ref_str(t: &TypeRef) -> String {
    match t {
        // v0.20a: function types render in Karn surface syntax.
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
            "karn-lsp-test-{}-{}",
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
                    "a.karn",
                    "commons demo.a\n\ntype Foo = Int where Positive\n",
                ),
                (
                    "b.karn",
                    "commons demo.b\n\nuses demo.a\n\ntype Bar = Int where NonNegative\n",
                ),
            ],
        );
        let current = Url::from_file_path(root.join("b.karn")).unwrap();
        let found = find_declaration_cross_file(&root, &current, "Foo")
            .expect("Foo should resolve into a.karn");
        let expected = Url::from_file_path(root.join("a.karn")).unwrap();
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
                "only.karn",
                "commons demo.only\n\ntype Foo = Int where Positive\n",
            )],
        );
        let current = Url::from_file_path(root.join("only.karn")).unwrap();
        // The only file containing Foo is current; cross-file must skip it.
        assert!(find_declaration_cross_file(&root, &current, "Foo").is_none());
    }

    #[test]
    fn cross_file_hover_returns_markdown_summary() {
        let root = setup_project(
            "cross_file_hover",
            &[
                (
                    "money.karn",
                    "commons demo.money\n\n\
                     ---\n\
                     Amount in minor units of currency.\n\
                     ---\n\
                     type Money = Int where NonNegative\n",
                ),
                (
                    "orders.karn",
                    "commons demo.orders\n\nuses demo.money\n\ntype OrderId = Int where Positive\n",
                ),
            ],
        );
        let current = Url::from_file_path(root.join("orders.karn")).unwrap();
        let (other_uri, desc) = describe_symbol_cross_file(&root, &current, "Money")
            .expect("Money should produce hover content");
        assert_eq!(
            other_uri,
            Url::from_file_path(root.join("money.karn")).unwrap()
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
                "a.karn",
                "commons demo.a\n\ntype Foo = Int where Positive\n",
            )],
        );
        let current = Url::from_file_path(root.join("a.karn")).unwrap();
        assert!(find_declaration_cross_file(&root, &current, "DoesNotExist").is_none());
        assert!(describe_symbol_cross_file(&root, &current, "DoesNotExist").is_none());
    }

    #[test]
    fn first_party_symbols_describe_their_signature_and_doc() {
        // Slice 9: stdlib/surface symbols live in the embedded sources, not the
        // project — the hover/completion-doc fallback finds them there, signature
        // and `---` doc block alike.
        let reverse = describe_firstparty_symbol("reverse").expect("`karn.list.reverse` described");
        assert!(
            reverse.contains("reverse") && reverse.contains("List"),
            "{reverse}"
        );
        assert!(
            reverse.contains("reverse order"),
            "doc block surfaced: {reverse}"
        );
        // The `karn` adapter surface too (a capability, exercising the adapter path).
        let clock = describe_firstparty_symbol("Clock").expect("`karn`-surface `Clock`");
        assert!(
            clock.contains("wall-clock"),
            "capability doc surfaced: {clock}"
        );
        // A name in no first-party source yields nothing (the fallback no-ops).
        assert!(describe_firstparty_symbol("DoesNotExist").is_none());
    }
}
