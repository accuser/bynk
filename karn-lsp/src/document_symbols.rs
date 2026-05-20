//! Document-symbol tree for the LSP `textDocument/documentSymbol` request
//! (v1.1; LSP spec §3.7).
//!
//! Walks a single file's parsed AST and emits a hierarchical
//! [`DocumentSymbol`] tree that populates VS Code's Outline pane and
//! powers "Go to Symbol in File" (Cmd-Shift-O). Multi-file commons /
//! contexts each report only their own file's contents — joining across
//! files is `workspaceSymbol` territory, which is deferred.

use karnc::ast::*;
use karnc::lexer::tokenize;
use karnc::parser::parse_unit_with_recovery;
use tower_lsp::lsp_types::{DocumentSymbol, Range, SymbolKind};

use crate::position::span_to_range;

/// Build the document-symbol tree for the given source text. Returns an
/// empty vector when the file cannot be parsed at all (no recognisable
/// header).
pub fn outline(source: &str) -> Vec<DocumentSymbol> {
    let Ok(tokens) = tokenize(source) else {
        return Vec::new();
    };
    let (unit, _errs) = parse_unit_with_recovery(&tokens, source);
    let Some(unit) = unit else {
        return Vec::new();
    };
    match unit {
        SourceUnit::Commons(c) => vec![commons_symbol(source, &c)],
        SourceUnit::Context(c) => vec![context_symbol(source, &c)],
        SourceUnit::Test(t) => vec![test_symbol(source, &t)],
    }
}

fn test_symbol(source: &str, t: &TestDecl) -> DocumentSymbol {
    let mut children: Vec<DocumentSymbol> = Vec::new();
    for m in &t.mocks {
        children.push(make_symbol(
            format!("mocks {} = {}", m.target_name.name, m.impl_name.name),
            None,
            SymbolKind::INTERFACE,
            span_to_range(source, m.span),
            span_to_range(source, m.target_name.span),
            Vec::new(),
        ));
    }
    for c in &t.cases {
        children.push(make_symbol(
            c.name.clone(),
            None,
            SymbolKind::FUNCTION,
            span_to_range(source, c.span),
            span_to_range(source, c.name_span),
            Vec::new(),
        ));
    }
    make_symbol(
        format!("test {}", t.target.joined()),
        detail_from_doc(&t.documentation),
        SymbolKind::MODULE,
        span_to_range(source, t.span),
        span_to_range(source, t.target.span),
        children,
    )
}

fn commons_symbol(source: &str, c: &Commons) -> DocumentSymbol {
    let children: Vec<DocumentSymbol> = c
        .items
        .iter()
        .map(|item| item_symbol(source, item))
        .collect();
    make_symbol(
        c.name.joined(),
        detail_from_doc(&c.documentation),
        SymbolKind::MODULE,
        span_to_range(source, c.span),
        span_to_range(source, c.name.span),
        children,
    )
}

fn context_symbol(source: &str, c: &Context) -> DocumentSymbol {
    let children: Vec<DocumentSymbol> = c
        .items
        .iter()
        .map(|item| item_symbol(source, item))
        .collect();
    make_symbol(
        c.name.joined(),
        detail_from_doc(&c.documentation),
        SymbolKind::MODULE,
        span_to_range(source, c.span),
        span_to_range(source, c.name.span),
        children,
    )
}

fn item_symbol(source: &str, item: &CommonsItem) -> DocumentSymbol {
    match item {
        CommonsItem::Type(t) => type_symbol(source, t),
        CommonsItem::Fn(f) => fn_symbol(source, f),
        CommonsItem::Capability(c) => capability_symbol(source, c),
        CommonsItem::Provider(p) => provider_symbol(source, p),
        CommonsItem::Service(s) => service_symbol(source, s),
        CommonsItem::Agent(a) => agent_symbol(source, a),
    }
}

fn type_symbol(source: &str, t: &TypeDecl) -> DocumentSymbol {
    let (kind, children) = match &t.body {
        TypeBody::Record(r) => (SymbolKind::STRUCT, record_field_symbols(source, &r.fields)),
        TypeBody::Sum(s) => (SymbolKind::ENUM, variant_symbols(source, &s.variants)),
        TypeBody::Opaque { .. } => (SymbolKind::CLASS, Vec::new()),
        TypeBody::Refined { .. } => (SymbolKind::TYPE_PARAMETER, Vec::new()),
    };
    make_symbol(
        t.name.name.clone(),
        detail_from_doc(&t.documentation),
        kind,
        span_to_range(source, t.span),
        span_to_range(source, t.name.span),
        children,
    )
}

fn record_field_symbols(source: &str, fields: &[RecordField]) -> Vec<DocumentSymbol> {
    fields
        .iter()
        .map(|f| {
            make_symbol(
                f.name.name.clone(),
                None,
                SymbolKind::FIELD,
                span_to_range(source, f.span),
                span_to_range(source, f.name.span),
                Vec::new(),
            )
        })
        .collect()
}

fn variant_symbols(source: &str, variants: &[Variant]) -> Vec<DocumentSymbol> {
    variants
        .iter()
        .map(|v| {
            make_symbol(
                v.name.name.clone(),
                None,
                SymbolKind::ENUM_MEMBER,
                span_to_range(source, v.span),
                span_to_range(source, v.name.span),
                Vec::new(),
            )
        })
        .collect()
}

fn fn_symbol(source: &str, f: &FnDecl) -> DocumentSymbol {
    // Free functions are top-level Function symbols. Methods (whose
    // owning type lives in the same file) would normally nest under that
    // type, but the type-decl symbol is built independently — see the
    // commons/context walk. For v1.1, surface methods as top-level
    // siblings with a "TypeName.method" name; nesting can be added once
    // the walker reorders items.
    let kind = match &f.name {
        FnName::Free(_) => SymbolKind::FUNCTION,
        FnName::Method { .. } => SymbolKind::METHOD,
    };
    make_symbol(
        f.name.display(),
        detail_from_doc(&f.documentation),
        kind,
        span_to_range(source, f.span),
        span_to_range(source, f.name.ident().span),
        Vec::new(),
    )
}

fn capability_symbol(source: &str, c: &CapabilityDecl) -> DocumentSymbol {
    let children = c
        .ops
        .iter()
        .map(|op| {
            make_symbol(
                op.name.name.clone(),
                detail_from_doc(&op.documentation),
                SymbolKind::METHOD,
                span_to_range(source, op.span),
                span_to_range(source, op.name.span),
                Vec::new(),
            )
        })
        .collect();
    make_symbol(
        c.name.name.clone(),
        detail_from_doc(&c.documentation),
        SymbolKind::INTERFACE,
        span_to_range(source, c.span),
        span_to_range(source, c.name.span),
        children,
    )
}

fn provider_symbol(source: &str, p: &ProviderDecl) -> DocumentSymbol {
    let children = p
        .ops
        .iter()
        .map(|op| {
            make_symbol(
                op.name.name.clone(),
                None,
                SymbolKind::METHOD,
                span_to_range(source, op.span),
                span_to_range(source, op.name.span),
                Vec::new(),
            )
        })
        .collect();
    // The display name shows both the capability and provider names so
    // the outline disambiguates multiple `provides X = ...` blocks.
    let name = format!("{} = {}", p.capability.name, p.provider_name.name);
    make_symbol(
        name,
        detail_from_doc(&p.documentation),
        SymbolKind::OBJECT,
        span_to_range(source, p.span),
        span_to_range(source, p.provider_name.span),
        children,
    )
}

fn service_symbol(source: &str, s: &ServiceDecl) -> DocumentSymbol {
    let children = s
        .handlers
        .iter()
        .map(|h| handler_symbol(source, h))
        .collect();
    make_symbol(
        s.name.name.clone(),
        detail_from_doc(&s.documentation),
        SymbolKind::CLASS,
        span_to_range(source, s.span),
        span_to_range(source, s.name.span),
        children,
    )
}

fn agent_symbol(source: &str, a: &AgentDecl) -> DocumentSymbol {
    let mut children = Vec::new();
    // Key field — surface as a Property.
    children.push(make_symbol(
        a.key_name.name.clone(),
        Some("key".into()),
        SymbolKind::PROPERTY,
        span_to_range(source, a.key_name.span),
        span_to_range(source, a.key_name.span),
        Vec::new(),
    ));
    // State fields.
    for field in &a.state_fields {
        children.push(make_symbol(
            field.name.name.clone(),
            Some("state".into()),
            SymbolKind::PROPERTY,
            span_to_range(source, field.span),
            span_to_range(source, field.name.span),
            Vec::new(),
        ));
    }
    // Handlers.
    for h in &a.handlers {
        children.push(handler_symbol(source, h));
    }
    make_symbol(
        a.name.name.clone(),
        detail_from_doc(&a.documentation),
        SymbolKind::CLASS,
        span_to_range(source, a.span),
        span_to_range(source, a.name.span),
        children,
    )
}

fn handler_symbol(source: &str, h: &Handler) -> DocumentSymbol {
    let name = match &h.method_name {
        Some(m) => format!("call {}", m.name),
        None => "call".to_string(),
    };
    let selection_span = h.method_name.as_ref().map(|m| m.span).unwrap_or(h.span);
    make_symbol(
        name,
        detail_from_doc(&h.documentation),
        SymbolKind::METHOD,
        span_to_range(source, h.span),
        span_to_range(source, selection_span),
        Vec::new(),
    )
}

fn detail_from_doc(doc: &Option<String>) -> Option<String> {
    doc.as_ref().and_then(|d| {
        let first = d
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())?
            .to_string();
        Some(first)
    })
}

#[allow(deprecated)] // `deprecated` and `tags` fields exist on DocumentSymbol.
fn make_symbol(
    name: String,
    detail: Option<String>,
    kind: SymbolKind,
    range: Range,
    selection_range: Range,
    children: Vec<DocumentSymbol>,
) -> DocumentSymbol {
    DocumentSymbol {
        name,
        detail,
        kind,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children: if children.is_empty() {
            None
        } else {
            Some(children)
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outline_of(src: &str) -> Vec<DocumentSymbol> {
        outline(src)
    }

    #[test]
    fn returns_empty_for_empty_input() {
        assert!(outline_of("").is_empty());
    }

    #[test]
    fn commons_with_types_and_fns_produces_module_with_children() {
        let src = "commons demo.x {\n\
                   type Money = Int where NonNegative\n\
                   fn double(n: Int) -> Int { n + n }\n\
                   }";
        let syms = outline_of(src);
        assert_eq!(syms.len(), 1);
        let module = &syms[0];
        assert_eq!(module.kind, SymbolKind::MODULE);
        assert_eq!(module.name, "demo.x");
        let children = module.children.as_ref().expect("children");
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].name, "Money");
        assert_eq!(children[0].kind, SymbolKind::TYPE_PARAMETER);
        assert_eq!(children[1].name, "double");
        assert_eq!(children[1].kind, SymbolKind::FUNCTION);
    }

    #[test]
    fn record_fields_nest_under_record_type() {
        let src = "commons demo.x {\n\
                   type Pt = { x: Int, y: Int }\n\
                   }";
        let syms = outline_of(src);
        let module = &syms[0];
        let children = module.children.as_ref().unwrap();
        let pt = &children[0];
        assert_eq!(pt.kind, SymbolKind::STRUCT);
        let fields = pt.children.as_ref().expect("record fields");
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name, "x");
        assert_eq!(fields[0].kind, SymbolKind::FIELD);
        assert_eq!(fields[1].name, "y");
    }

    #[test]
    fn sum_variants_nest_under_enum() {
        let src = "commons demo.x {\n\
                   type Tag = enum { Foo, Bar, Baz }\n\
                   }";
        let syms = outline_of(src);
        let module = &syms[0];
        let tag = &module.children.as_ref().unwrap()[0];
        assert_eq!(tag.kind, SymbolKind::ENUM);
        let variants = tag.children.as_ref().expect("variants");
        assert_eq!(variants.len(), 3);
        assert_eq!(variants[0].kind, SymbolKind::ENUM_MEMBER);
        assert_eq!(variants[2].name, "Baz");
    }

    #[test]
    fn opaque_type_uses_class_kind() {
        let src = "commons demo.x {\n\
                   type Id = opaque Int where NonNegative\n\
                   }";
        let syms = outline_of(src);
        let id = &syms[0].children.as_ref().unwrap()[0];
        assert_eq!(id.kind, SymbolKind::CLASS);
    }

    #[test]
    fn context_with_service_and_agent_produces_hierarchical_tree() {
        let src = "context demo.app {\n\
                   capability Clock { fn now() -> Int }\n\
                   service Api {\n\
                   on call(amount: Int) -> Int given Clock { amount }\n\
                   }\n\
                   agent Counter {\n\
                   key id: Int\n\
                   state { value: Int }\n\
                   on call bump(by: Int) -> Int { 0 }\n\
                   }\n\
                   }";
        let syms = outline_of(src);
        let module = &syms[0];
        assert_eq!(module.name, "demo.app");
        let children = module.children.as_ref().unwrap();
        // capability + service + agent
        let kinds: Vec<SymbolKind> = children.iter().map(|c| c.kind).collect();
        assert!(kinds.contains(&SymbolKind::INTERFACE));
        assert!(kinds.contains(&SymbolKind::CLASS));
        let service = children
            .iter()
            .find(|c| c.name == "Api")
            .expect("Api service");
        let service_children = service.children.as_ref().unwrap();
        assert_eq!(service_children.len(), 1);
        assert_eq!(service_children[0].kind, SymbolKind::METHOD);
        let agent = children
            .iter()
            .find(|c| c.name == "Counter")
            .expect("Counter agent");
        let agent_children = agent.children.as_ref().unwrap();
        // key + state field + handler = 3 children
        assert_eq!(agent_children.len(), 3);
        assert!(
            agent_children
                .iter()
                .any(|c| c.kind == SymbolKind::PROPERTY && c.name == "value")
        );
        assert!(
            agent_children
                .iter()
                .any(|c| c.kind == SymbolKind::METHOD && c.name == "call bump")
        );
    }

    #[test]
    fn doc_block_first_line_appears_as_detail() {
        let src = "commons demo.x {\n\
                   ---\n\
                   Short one-liner.\n\
                   Second line.\n\
                   ---\n\
                   type T = Int where Positive\n\
                   }";
        let syms = outline_of(src);
        let t = &syms[0].children.as_ref().unwrap()[0];
        assert_eq!(t.detail.as_deref(), Some("Short one-liner."));
    }
}
