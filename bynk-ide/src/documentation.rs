//! #847: the documentation-model query — a file's declarations aggregated into
//! a rendered reference page ("live rustdoc for Bynk"), for the VS Code
//! "Show Documentation" webview.
//!
//! A pure, read-only IDE query. It does two things and reuses everything else:
//!
//! - **Traversal.** It walks the parsed unit's `items` with an *exhaustive*
//!   match on [`CommonsItem`] — the same shape `bynk-lsp`'s `document_symbols`
//!   walk uses — so a new declaration kind is a compile error here, in
//!   `push_item`, not a silently-missed row on the page. (Risk: "a new
//!   doc-bearing node kind is added and silently missed by the aggregator.")
//!
//! - **Per-declaration rendering.** Each entry's Markdown (its fenced signature
//!   plus its doc-comment prose) is produced by hover's own `describe_*`
//!   assembly in [`crate::symbols`] — not a parallel renderer. Sharing that code
//!   path is what keeps the doc page from drifting from hover. (Risk:
//!   "divergence from hover — two code paths formatting the same doc
//!   differently.")
//!
//! What this module adds on top is the page *structure*: the declaration's
//! heading name, its nesting depth (top-level item → its ops/handlers), a
//! `documented` flag driving the "no documentation" coverage placeholder, and
//! the name span each heading links back to (click-to-code). The Markdown is
//! rendered — HTML-disabled — by the webview; nothing here emits HTML.
//!
//! Tier 1 (Decision A) is **file-scoped**: the model is built from one file's
//! text, exactly like `document_symbols`. Context-aggregation (merging every
//! file of a multi-file `context`) is the deferred follow-up.

use bynk_syntax::ast::*;
use bynk_syntax::lexer::tokenize;
use bynk_syntax::parser::parse_unit_with_recovery;
use bynk_syntax::span::Span;

use crate::symbols;

/// A file's declarations rendered as an ordered, hierarchical reference page.
#[derive(Debug, Clone, PartialEq)]
pub struct DocModel {
    /// The declared unit name (the page title) — `demo.app` for a
    /// `context demo.app`, `tokens` for an `adapter tokens`.
    pub unit_name: String,
    /// `"commons"` / `"context"` / `"adapter"` — the unit's own keyword, shown
    /// as the page's kind.
    pub unit_kind: &'static str,
    /// The unit declaration's own doc comment, if any (rendered above the
    /// entries as the page's lede).
    pub unit_doc: Option<String>,
    /// The unit-name span — the page title links back to the header.
    pub unit_span: Span,
    /// Declarations in source order, each carrying its nesting `depth`.
    pub entries: Vec<DocEntry>,
}

/// One declaration on the page: a heading, its rendered signature+doc Markdown,
/// and where it lives in the source.
#[derive(Debug, Clone, PartialEq)]
pub struct DocEntry {
    /// The heading text — a bare name (`Api`), a compound member key
    /// (`Counter.bump`, `Clock.now`), or a provider's `Cap = Provider`.
    pub name: String,
    /// A short kind label for the heading badge (`"service"`, `"handler"`, …).
    pub kind: &'static str,
    /// Nesting level: top-level declarations are `0`; a capability's ops and a
    /// service/agent's handlers are `1`.
    pub depth: u32,
    /// The declaration's Markdown — a fenced `bynk` signature, followed by its
    /// doc-comment prose when documented. Produced by hover's `describe_*`
    /// (see the module doc).
    pub markdown: String,
    /// Whether this declaration carries a doc comment. Drives the webview's
    /// "no documentation" placeholder (Decision B: the page doubles as a
    /// doc-coverage view, with a toggle to hide the undocumented).
    pub documented: bool,
    /// The declaration's name span — the heading and signature link here.
    pub span: Span,
}

/// Build the documentation model for a single file's `text`. Returns `None`
/// when the file has no recognisable unit header, or is a test suite (`suite`
/// units are not a documentation unit in Tier 1 — their `case`/`stub` members
/// have no `describe_*` renderer, and a doc page for tests is out of scope).
pub fn documentation_model(text: &str) -> Option<DocModel> {
    let tokens = tokenize(text).ok()?;
    let (unit, _errs) = parse_unit_with_recovery(&tokens, text);
    let unit = unit?;
    let (unit_kind, unit_name, unit_span, unit_doc, items) = match &unit {
        SourceUnit::Commons(c) => (
            "commons",
            c.name.joined(),
            c.name.span,
            &c.documentation,
            &c.items,
        ),
        SourceUnit::Context(c) => (
            "context",
            c.name.joined(),
            c.name.span,
            &c.documentation,
            &c.items,
        ),
        SourceUnit::Adapter(a) => (
            "adapter",
            a.name.joined(),
            a.name.span,
            &a.documentation,
            &a.items,
        ),
        SourceUnit::Suite(_) => return None,
    };
    let mut entries = Vec::new();
    for item in items {
        push_item(&mut entries, item);
    }
    Some(DocModel {
        unit_name,
        unit_kind,
        unit_doc: unit_doc.clone(),
        unit_span,
        entries,
    })
}

/// Append `item`'s entry — and any nested member entries (capability ops,
/// service/agent handlers) — to `out`, in source order.
///
/// The `match` is exhaustive over [`CommonsItem`] on purpose: a new item kind
/// will not compile until it is given a page entry, which is the guard against
/// the "silently-missed declaration kind" risk. Nested members reuse the same
/// discipline against their own child lists.
fn push_item(out: &mut Vec<DocEntry>, item: &CommonsItem) {
    match item {
        CommonsItem::Type(t) => out.push(DocEntry {
            name: t.name.name.clone(),
            kind: "type",
            depth: 0,
            markdown: symbols::describe_type(t),
            documented: t.documentation.is_some(),
            span: t.name.span,
        }),
        CommonsItem::Fn(f) => out.push(DocEntry {
            name: f.name.display(),
            kind: match f.name {
                FnName::Free(_) => "function",
                FnName::Method { .. } => "method",
            },
            depth: 0,
            markdown: symbols::describe_fn(f),
            documented: f.documentation.is_some(),
            span: f.name.ident().span,
        }),
        CommonsItem::Capability(c) => {
            out.push(DocEntry {
                name: c.name.name.clone(),
                kind: "capability",
                depth: 0,
                markdown: symbols::describe_capability(c),
                documented: c.documentation.is_some(),
                span: c.name.span,
            });
            for op in &c.ops {
                out.push(DocEntry {
                    name: format!("{}.{}", c.name.name, op.name.name),
                    kind: "operation",
                    depth: 1,
                    markdown: symbols::describe_capability_op(c, op),
                    documented: op.documentation.is_some(),
                    span: op.name.span,
                });
            }
        }
        CommonsItem::Provider(p) => out.push(DocEntry {
            name: format!("{} = {}", p.capability.name, p.provider_name.name),
            kind: "provider",
            depth: 0,
            markdown: symbols::describe_provider(p),
            documented: p.documentation.is_some(),
            span: p.provider_name.span,
        }),
        CommonsItem::Service(s) => {
            out.push(DocEntry {
                name: s.name.name.clone(),
                kind: "service",
                depth: 0,
                markdown: symbols::describe_service(s),
                documented: s.documentation.is_some(),
                span: s.name.span,
            });
            for h in &s.handlers {
                out.push(DocEntry {
                    // A service handler is identified by its route, not a
                    // dispatch name (`on GET("/x")`), so the heading is the
                    // route line itself.
                    name: symbols::handler_line(h),
                    kind: "handler",
                    depth: 1,
                    markdown: symbols::describe_service_handler(s, h),
                    documented: h.documentation.is_some(),
                    span: h.span,
                });
            }
        }
        CommonsItem::Agent(a) => {
            out.push(DocEntry {
                name: a.name.name.clone(),
                kind: "agent",
                depth: 0,
                markdown: symbols::describe_agent(a),
                documented: a.documentation.is_some(),
                span: a.name.span,
            });
            for h in &a.handlers {
                let handler = h
                    .method_name
                    .as_ref()
                    .map(|m| m.name.clone())
                    .unwrap_or_else(|| "call".to_string());
                out.push(DocEntry {
                    name: format!("{}.{}", a.name.name, handler),
                    kind: "handler",
                    depth: 1,
                    markdown: symbols::describe_agent_handler(a, h, &handler),
                    documented: h.documentation.is_some(),
                    span: h.method_name.as_ref().map(|m| m.span).unwrap_or(h.span),
                });
            }
        }
        CommonsItem::Actor(a) => out.push(DocEntry {
            name: a.name.name.clone(),
            kind: "actor",
            depth: 0,
            markdown: symbols::describe_actor(a),
            documented: a.documentation.is_some(),
            span: a.name.span,
        }),
        // message-bundles slice 1 (#859): a messages block, keyed by its tag.
        CommonsItem::Messages(m) => out.push(DocEntry {
            name: m.tag.name.clone(),
            kind: "messages",
            depth: 0,
            markdown: symbols::describe_messages(m),
            documented: m.documentation.is_some(),
            span: m.tag.span,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fixture: a fully-documented context with a nested hierarchy — a
    /// capability with an op, a service with a handler, and an agent with a
    /// handler. Asserts every declaration appears, in source order, with its
    /// doc rendered and its depth reflecting the hierarchy.
    const DOCUMENTED_CTX: &str = r#"---
The demo application context.
---
context demo.app

---
Reads the wall clock.
---
capability Clock {
  ---
  Milliseconds since the Unix epoch.
  ---
  fn now() -> Int
}

---
An amount in the smallest currency unit.
---
type Money = Int where NonNegative

---
The public HTTP surface.
---
service Api from http {
  ---
  Returns the current instant.
  ---
  on GET("/now") () -> Effect[Int] given Clock {
    Clock.now()
  }
}

---
A per-key running total.
---
agent Counter {
  key id: Int
  store value: Cell[Int]
  ---
  Adds `amount` to the total and returns the new value.
  ---
  on call bump(amount: Int) -> Effect[Int] {
    let _ <- value.update((v) => v + amount)
    value
  }
}
"#;

    #[test]
    fn documented_context_aggregates_every_declaration_in_order_and_hierarchy() {
        let model = documentation_model(DOCUMENTED_CTX).expect("a context model");
        assert_eq!(model.unit_name, "demo.app");
        assert_eq!(model.unit_kind, "context");
        assert_eq!(
            model.unit_doc.as_deref(),
            Some("The demo application context.")
        );

        // Names, in source order, with the nested members interleaved after
        // their owners.
        let names: Vec<(&str, u32, &str)> = model
            .entries
            .iter()
            .map(|e| (e.name.as_str(), e.depth, e.kind))
            .collect();
        assert_eq!(
            names,
            vec![
                ("Clock", 0, "capability"),
                ("Clock.now", 1, "operation"),
                ("Money", 0, "type"),
                ("Api", 0, "service"),
                ("on GET(\"/now\")", 1, "handler"),
                ("Counter", 0, "agent"),
                ("Counter.bump", 1, "handler"),
            ]
        );

        // Every declaration here is documented, and the doc prose reaches the
        // rendered Markdown (proving the reuse of hover's assembly).
        assert!(model.entries.iter().all(|e| e.documented));
        let money = model.entries.iter().find(|e| e.name == "Money").unwrap();
        assert!(
            money
                .markdown
                .contains("An amount in the smallest currency unit.")
        );
        assert!(money.markdown.contains("```bynk"));
        let bump = model
            .entries
            .iter()
            .find(|e| e.name == "Counter.bump")
            .unwrap();
        assert!(bump.markdown.contains("Adds `amount` to the total"));
    }

    #[test]
    fn undocumented_declarations_are_flagged_for_the_coverage_placeholder() {
        let src = "commons demo.x {\n\
                   ---\n\
                   Has docs.\n\
                   ---\n\
                   type Documented = Int\n\
                   type Undocumented = Int\n\
                   fn helper(n: Int) -> Int { n }\n\
                   }";
        let model = documentation_model(src).expect("a commons model");
        assert_eq!(model.unit_kind, "commons");
        let documented = model
            .entries
            .iter()
            .find(|e| e.name == "Documented")
            .unwrap();
        assert!(documented.documented);
        let undoc = model
            .entries
            .iter()
            .find(|e| e.name == "Undocumented")
            .unwrap();
        assert!(!undoc.documented);
        // An undocumented declaration still renders its signature — the page is
        // a reference, not only a comment dump (Decision C).
        assert!(undoc.markdown.contains("```bynk"));
        let helper = model.entries.iter().find(|e| e.name == "helper").unwrap();
        assert!(!helper.documented);
    }

    #[test]
    fn suite_units_have_no_documentation_page() {
        let src = "suite for demo.app {\n\
                   case works {\n\
                   expect true\n\
                   }\n\
                   }";
        // A `suite` unit is not a documentation unit in Tier 1.
        assert!(documentation_model(src).is_none());
    }

    #[test]
    fn empty_input_yields_no_model() {
        assert!(documentation_model("").is_none());
    }

    #[test]
    fn adapter_unit_documents_its_items() {
        let src = "adapter tokens {\n\
                   binding \"./tokens.binding.ts\"\n\
                   exports capability { Jwt }\n\
                   capability Jwt {\n\
                   fn sign(secret: String) -> Effect[String]\n\
                   }\n\
                   provides Jwt = JoseJwt\n\
                   }";
        let model = documentation_model(src).expect("an adapter model");
        assert_eq!(model.unit_name, "tokens");
        assert_eq!(model.unit_kind, "adapter");
        assert!(
            model
                .entries
                .iter()
                .any(|e| e.name == "Jwt" && e.kind == "capability")
        );
        assert!(
            model
                .entries
                .iter()
                .any(|e| e.name == "Jwt.sign" && e.kind == "operation")
        );
        assert!(
            model
                .entries
                .iter()
                .any(|e| e.name == "Jwt = JoseJwt" && e.kind == "provider")
        );
    }
}
