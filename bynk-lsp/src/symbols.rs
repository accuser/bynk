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

/// v0.137.0 (ADR 0161): hover for the `key`/`store` *contextual* keywords and
/// the agent state fields they introduce. Both are lexed as `Ident`s (not
/// reserved `KEYWORDS`), and the fields they declare are neither `let`/param
/// locals nor top-level declarations — so neither the keyword fallback nor the
/// `describe_symbol`/locals paths in the hover handler reach them. This closes
/// that gap: for the cursor on the `key`/`store` keyword *or* on the field name
/// it declares, render the field's signature (type, and a `store` field's
/// `@indexed`/`@bounded`/… annotations) followed by the contextual-keyword doc.
///
/// #611 (gap A): a *reference* to a state field inside the agent's body — a
/// bare read (`lastSeq + 1`), a `:=` write target, an invariant subject, a store
/// op's receiver (`items.put(…)`) — renders the same hover as its declaration.
/// State fields are absent from the project index and are not `let`/param
/// locals, so a reference resolved nowhere before this. The hover handler tries
/// the locals path first, so a local shadowing a field name still hovers as the
/// local — matching the checker, which dispatches a store op only on a bare
/// ident that is *not* in the value scope.
///
/// `None` when the cursor is not on an agent's `key`/`store` keyword, its
/// state-field name, or a reference to one within the agent.
pub fn describe_agent_state_at(source: &str, offset: usize) -> Option<String> {
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
        let CommonsItem::Agent(a) = item else {
            continue;
        };
        // `key <name>: <type>` — the cursor on the field name, or on the `key`
        // keyword token immediately preceding it.
        let on_key_kw = preceding_ident_span(&tokens, source, a.key_name.span, "key")
            .is_some_and(|s| span_covers(s, offset));
        if on_key_kw || span_covers(a.key_name.span, offset) {
            return Some(key_hover(a));
        }
        // `store <name>: <kind> <annotations>` — the parser sets each field's
        // span to start at its `store` keyword, so the keyword span is derivable
        // without re-scanning.
        for f in &a.store_fields {
            let store_kw = Span {
                start: f.span.start,
                end: f.span.start + "store".len(),
            };
            let on_store_kw = source.get(store_kw.start..store_kw.end) == Some("store")
                && span_covers(store_kw, offset);
            if on_store_kw || span_covers(f.name.span, offset) {
                return Some(store_field_hover(f));
            }
        }
        // #611: a reference to `key`/`store` state from within this agent.
        if !in_state_scope(a, offset) {
            continue;
        }
        let Some((name, name_span)) = ident_at(&tokens, source, offset) else {
            continue;
        };
        // State is referenced by **bare** name, so a member of another value
        // (`p.items`) is not a state reference even when the names coincide.
        if is_dot_preceded(source, name_span.start) {
            continue;
        }
        if name == a.key_name.name {
            return Some(key_hover(a));
        }
        if let Some(f) = a.store_fields.iter().find(|f| f.name.name == name) {
            return Some(store_field_hover(f));
        }
    }
    None
}

/// #611: true when `offset` sits where an agent's `key`/`store` state is
/// referenceable by bare name — a handler body, or an invariant/transition
/// predicate. Deliberately narrower than the agent's own span: the declaration
/// region names things that are *not* state references, and a store annotation
/// argument (`@indexed(by: id)`) names a field of the **stored value**, which
/// must not masquerade as a same-named `key`/`store` field.
fn in_state_scope(a: &AgentDecl, offset: usize) -> bool {
    a.handlers.iter().any(|h| span_covers(h.body.span, offset))
        || a.invariants
            .iter()
            .any(|i| span_covers(i.predicate.span, offset))
        || a.transitions
            .iter()
            .any(|t| span_covers(t.predicate.span, offset))
}

/// The hover for an agent's `key` field — its declaration and every reference.
fn key_hover(a: &AgentDecl) -> String {
    let sig = format!("key {}: {}", a.key_name.name, type_ref_str(&a.key_type));
    render_state_hover(&sig, "key")
}

/// The hover for a `store` field — its declaration and every reference.
fn store_field_hover(f: &StoreField) -> String {
    let mut sig = format!("store {}: {}", f.name.name, store_kind_str(&f.kind));
    for ann in &f.annotations {
        sig.push(' ');
        sig.push_str(&bynk_fmt::annotation_to_string(ann));
    }
    render_state_hover(&sig, "store")
}

/// #611: hover for a `store` field's operation — the `<op>` of a
/// `<field>.<op>(…)` call on an agent's `store` field (`items.put(id, item)`).
/// Store operations are checked but never indexed and are not value-receiver
/// methods, so `qualified_callee_at` (name-receivers only) never reaches them
/// and they resolved nowhere. Renders the operation's signature from the
/// enumerable [`bynk_check::store_ops`] registry — generic in the kind's
/// key/value/element type — over the field's declared kind, which grounds it.
///
/// `locals` guards the receiver the way the checker's dispatch does: a store op
/// is a bare ident receiver that is *not* in the value scope, so a local
/// shadowing the field name makes this an ordinary value method, not a store op.
/// `None` when the cursor is not on a store operation of the enclosing agent.
pub(crate) fn describe_store_op_at(
    source: &str,
    offset: usize,
    locals: &[bynk_check::locals::LocalBinding],
) -> Option<String> {
    let tokens = tokenize(source).ok()?;
    let (unit, _errs) = parse_unit_with_recovery(&tokens, source);
    let unit = unit?;
    let items: &[CommonsItem] = match &unit {
        SourceUnit::Commons(c) => &c.items,
        SourceUnit::Context(c) => &c.items,
        SourceUnit::Adapter(a) => &a.items,
        SourceUnit::Suite(_) => &[],
    };
    // The cursor must sit on the `<op>` of a `<recv>.<op>` access.
    let (op, op_span) = ident_at(&tokens, source, offset)?;
    let (recv, recv_start) = receiver_segment_at(source, op_span)?;
    // The checker dispatches a store op on a **bare** ident receiver only, so a
    // qualified one is not one: `p.items.contains(…)` is an ordinary value method
    // on a record field that merely shares a store field's name.
    if is_dot_preceded(source, recv_start) {
        return None;
    }
    // A local of the receiver's name shadows the store field (the same
    // by-provenance dispatch) — then this is a value method, not a store op.
    if bynk_check::locals::locals_at(locals, recv_start)
        .iter()
        .any(|b| b.name == recv)
    {
        return None;
    }
    for item in items {
        let CommonsItem::Agent(a) = item else {
            continue;
        };
        if !in_state_scope(a, offset) {
            continue;
        }
        let f = a.store_fields.iter().find(|f| f.name.name == recv)?;
        let sig = bynk_check::store_ops::ops_for(&f.kind.head.name)
            .iter()
            .find(|o| o.name == op)?
            .signature;
        return Some(format!(
            "```bynk\n{sig}\n```\n\nA `{}` store operation on `store {}: {}` — the field's \
             declared kind grounds the operation's type parameters.",
            f.kind.head.name,
            f.name.name,
            store_kind_str(&f.kind),
        ));
    }
    None
}

/// The identifier token covering `offset` — its text and span — if the cursor is
/// on one.
fn ident_at<'a>(
    tokens: &[bynk_syntax::lexer::Token],
    source: &'a str,
    offset: usize,
) -> Option<(&'a str, Span)> {
    tokens
        .iter()
        .find(|t| t.kind == bynk_syntax::lexer::TokenKind::Ident && span_covers(t.span, offset))
        .and_then(|t| Some((source.get(t.span.start..t.span.end)?, t.span)))
}

/// The receiver segment of a `<recv>.<member>` access whose member sits at
/// `member_span`: the identifier run immediately before the dot, and the offset
/// it starts at. `None` when the member is not dot-preceded. Shared by every
/// caller that reads a receiver off the line prefix, so the extraction has one
/// definition rather than a copy per call site.
fn receiver_segment_at(text: &str, member_span: Span) -> Option<(&str, usize)> {
    let before = text.get(..member_span.start)?.strip_suffix('.')?;
    let start = before
        .rfind(|c: char| !(c.is_alphanumeric() || c == '_'))
        .map_or(0, |i| i + 1);
    Some((&before[start..], start))
}

/// True when the identifier starting at `start` is itself the member of a
/// further access (the `items` of `p.items`) rather than a bare name.
fn is_dot_preceded(text: &str, start: usize) -> bool {
    text[..start].ends_with('.')
}

/// v0.140 (ADR 0163): hover for a handler-position annotation (`@cache`). Handler
/// annotations are not symbols and declare no local, so they miss both the
/// `describe_symbol` and locals paths — this closes the gap. For the cursor
/// anywhere within a handler's `@cache( … )` annotation, render the formatted
/// annotation followed by a prose description of `@cache` and its fields. `None`
/// when the cursor is not inside a handler annotation.
pub fn describe_handler_annotation_at(source: &str, offset: usize) -> Option<String> {
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
        let handlers: &[Handler] = match item {
            CommonsItem::Service(s) => &s.handlers,
            CommonsItem::Agent(a) => &a.handlers,
            _ => continue,
        };
        for h in handlers {
            for ann in &h.annotations {
                if span_covers(ann.span, offset) {
                    return Some(render_handler_annotation_hover(ann));
                }
            }
        }
    }
    None
}

/// v0.140 (ADR 0163): the spans to classify as `decorator` semantic tokens — each
/// handler annotation's `@name` (the `@` through the name) and its argument labels
/// (`maxAge:`, `scope:`). Parsed from `source`; empty when it carries no handler
/// annotations. Feeds the semantic-tokens producer, which is otherwise a
/// parse-free index read, so the parse lives here beside the hover parse.
pub fn handler_annotation_token_spans(source: &str) -> Vec<Span> {
    let Ok(tokens) = tokenize(source) else {
        return Vec::new();
    };
    let (unit, _errs) = parse_unit_with_recovery(&tokens, source);
    let Some(unit) = unit else {
        return Vec::new();
    };
    let items: &[CommonsItem] = match &unit {
        SourceUnit::Commons(c) => &c.items,
        SourceUnit::Context(c) => &c.items,
        SourceUnit::Adapter(a) => &a.items,
        SourceUnit::Suite(_) => &[],
    };
    let mut spans = Vec::new();
    for item in items {
        let handlers: &[Handler] = match item {
            CommonsItem::Service(s) => &s.handlers,
            CommonsItem::Agent(a) => &a.handlers,
            _ => continue,
        };
        for h in handlers {
            for ann in &h.annotations {
                // The `@name` — from the annotation's leading `@` through its name.
                spans.push(Span {
                    start: ann.span.start,
                    end: ann.name.span.end,
                });
                // Each argument label (`maxAge:`, `scope:`).
                for arg in &ann.args {
                    if let Some(label) = &arg.label {
                        spans.push(label.span);
                    }
                }
            }
        }
    }
    spans
}

/// The formatted annotation in a code block, plus a prose description for the
/// closed handler-annotation set. `@cache` and `@limit` (v0.142) carry prose; any
/// other name (a typo the checker will flag) still hovers as its formatted form so
/// the surface is never silent.
fn render_handler_annotation_hover(ann: &Annotation) -> String {
    let sig = bynk_fmt::annotation_to_string(ann);
    if ann.name.name == "cache" {
        return format!(
            "```bynk\n{sig}\n```\n\n\
             **`@cache`** — cache this `GET` read. Every eligible `GET` already carries a \
             synthesised weak `ETag` and is answered `304 Not Modified` on a matching \
             `If-None-Match`; `@cache` adds a `Cache-Control` freshness window on top.\n\n\
             - **`maxAge`** — the freshness window, a `Duration` (e.g. `5.minutes`), lowered to \
             `Cache-Control: max-age`.\n\
             - **`scope`** — `public` or `private` (default `private`; a shared cache stores the \
             response only when `public`)."
        );
    }
    // v0.142 (ADR 0165): `@limit` caps the request body size on a write route.
    if ann.name.name == "limit" {
        return format!(
            "```bynk\n{sig}\n```\n\n\
             **`@limit`** — cap the request body size on this `POST`/`PUT`/`PATCH` route. A \
             request whose body exceeds the `maxBody` byte ceiling is answered `413 Payload Too \
             Large`, synthesised before the body is read.\n\n\
             - **`maxBody`** — the maximum request body size in bytes, a positive `Int`."
        );
    }
    format!("```bynk\n{sig}\n```")
}

/// True when `offset` falls within `span` (half-open, as hover offsets are).
fn span_covers(span: Span, offset: usize) -> bool {
    span.start <= offset && offset < span.end
}

/// The span of the token immediately preceding the token that begins at
/// `name_span`, if that preceding token's source text is exactly `kw` — used to
/// locate a contextual keyword (`key`) that the AST records only by its effect,
/// not with a span of its own.
fn preceding_ident_span(
    tokens: &[bynk_syntax::lexer::Token],
    source: &str,
    name_span: Span,
    kw: &str,
) -> Option<Span> {
    let idx = tokens
        .iter()
        .position(|t| t.span.start == name_span.start)?;
    let prev = tokens.get(idx.checked_sub(1)?)?;
    (source.get(prev.span.start..prev.span.end) == Some(kw)).then_some(prev.span)
}

/// A code-block field signature followed by the contextual keyword's one-line
/// doc from [`bynk_syntax::keywords::CONTEXTUAL_KEYWORDS`].
fn render_state_hover(sig: &str, contextual_kw: &str) -> String {
    let doc = bynk_syntax::keywords::CONTEXTUAL_KEYWORDS
        .iter()
        .find(|k| k.word == contextual_kw)
        .map(|k| k.meaning)
        .unwrap_or_default();
    format!("```bynk\n{sig}\n```\n\n{doc}")
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
    let (recv, _) = receiver_segment_at(text, ident_span)?;
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
    let mut out: Vec<(String, Span)> = Vec::new();
    // A suite links its target (the unit under test) plus any `uses` clauses,
    // mirroring the `uses`/`consumes` links on the other unit kinds (#609).
    let (uses, consumes): (&[UsesDecl], &[ConsumesDecl]) = match &unit {
        SourceUnit::Commons(c) => (&c.uses, &[]),
        SourceUnit::Context(c) => (&c.uses, &c.consumes),
        SourceUnit::Adapter(a) => (&a.uses, &a.consumes),
        SourceUnit::Suite(s) => {
            out.push((s.target.joined(), s.target.span));
            (&s.uses, &[])
        }
    };
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
        // v0.166 (#616): a bare key names a *free* function. A method's identity
        // is its compound `"Type.method"` key (below); matching one by its bare
        // method name answered with whichever type declared it first, so
        // `g.bump()` and even `fn Gauge.bump`'s own declaration rendered
        // `Counter.bump`. `signature_help::resolve_label` guards its free-fn path
        // the same way.
        CommonsItem::Fn(f) if matches!(f.name, FnName::Free(_)) && f.name.ident().name == name => {
            Some(describe_fn(f))
        }
        CommonsItem::Capability(c) if c.name.name == name => Some(describe_capability(c)),
        CommonsItem::Service(s) if s.name.name == name => Some(describe_service(s)),
        CommonsItem::Agent(a) if a.name.name == name => Some(describe_agent(a)),
        CommonsItem::Provider(p) if p.provider_name.name == name => Some(describe_provider(p)),
        // v0.166 (#616): an actor, keyed by its plain name — the `Actor` index
        // kind ADR 0190 filed as the clearest evidence that the renderer, not the
        // ladder, is where these were missing. `by u: User` resolved here and
        // rendered nothing.
        CommonsItem::Actor(a) if a.name.name == name => Some(describe_actor(a)),
        // #611 (gap B): a record field, keyed `"Type.field"` by the index — the
        // checker records construction labels and field accesses as `Field` refs,
        // so hover resolves the key but had no arm to render it and fell through
        // to the locals path, which name-matches in scope (a `title:` label bound
        // to a same-named handler param). Top-level names carry no `.`, so the
        // compound key can only match here.
        CommonsItem::Type(t) => {
            let (owner, field) = name.rsplit_once('.')?;
            if t.name.name != owner {
                return None;
            }
            let TypeBody::Record(r) = &t.body else {
                return None;
            };
            r.fields
                .iter()
                .find(|f| f.name.name == field)
                .map(|f| describe_record_field(t, f))
        }
        // v0.166 (#616): a method, keyed `"Type.method"` (ADR 0069). `display()`
        // renders exactly that key, so the compound name matches the one method
        // it names — the type prefix is what disambiguates `Counter.bump` from
        // `Gauge.bump`.
        CommonsItem::Fn(f) => (f.name.display() == name).then(|| describe_fn(f)),
        // v0.166 (#616): a capability operation, keyed `"Cap.op"` (ADR 0069).
        CommonsItem::Capability(c) => {
            let (owner, op) = name.rsplit_once('.')?;
            if c.name.name != owner {
                return None;
            }
            c.ops
                .iter()
                .find(|o| o.name.name == op)
                .map(|o| describe_capability_op(c, o))
        }
        _ => None,
    }
}

/// v0.166 (#616): an actor as declared — the `auth` scheme with its config, the
/// `identity` type, or the refinement form's base and claim predicate.
/// Mirrors `bynk-fmt`'s `format_actor`, as [`describe_agent`] mirrors an agent.
fn describe_actor(a: &ActorDecl) -> String {
    let mut out = String::from("```bynk\n");
    match &a.refinement {
        // `actor Admin = User where hasClaim("admin")` (ADR 0091).
        Some(r) => out.push_str(&format!(
            "actor {} = {} where {}",
            a.name.name,
            r.base.name,
            bynk_fmt::expr_to_string(&r.predicate)
        )),
        None => {
            // An absent `auth` is the `None` scheme, which is how it parses.
            let auth = a.auth.as_ref().map_or("None", |i| i.name.as_str());
            out.push_str(&format!("actor {} {{ auth = {auth}", a.name.name));
            if !a.auth_config.is_empty() {
                let args: Vec<String> = a
                    .auth_config
                    .iter()
                    .map(|arg| match &arg.value {
                        // The parser resolves escapes at lex time, so the stored
                        // value is *unescaped* — re-escape it through the
                        // formatter's own escaper, or a `"` in the config renders
                        // as invalid Bynk inside the fence below.
                        SchemeArgValue::Str(s) => {
                            format!("{} = \"{}\"", arg.key.name, bynk_fmt::escape_string(s))
                        }
                        SchemeArgValue::Int(n) => format!("{} = {n}", arg.key.name),
                    })
                    .collect();
                out.push_str(&format!("({})", args.join(", ")));
            }
            if let Some(id) = &a.identity {
                out.push_str(&format!(", identity = {}", type_ref_str(id)));
            }
            out.push_str(" }");
        }
    }
    out.push_str("\n```\n");
    if let Some(doc) = &a.documentation {
        out.push('\n');
        out.push_str(doc);
        out.push('\n');
    }
    out
}

/// v0.166 (#616): a capability operation as declared, attributed to the
/// capability that owns it. Mirrors how [`describe_capability`] renders the same
/// op within the capability body, as [`describe_record_field`] does for a field.
fn describe_capability_op(c: &CapabilityDecl, op: &CapabilityOp) -> String {
    let params: Vec<String> = op
        .params
        .iter()
        .map(|p| format!("{}: {}", p.name.name, type_ref_str(&p.type_ref)))
        .collect();
    let mut out = format!(
        "```bynk\nfn {}({}) -> {}\n```\n\nAn operation of capability `{}`.\n",
        op.name.name,
        params.join(", "),
        type_ref_str(&op.return_type),
        c.name.name
    );
    if let Some(doc) = &op.documentation {
        out.push('\n');
        out.push_str(doc);
        out.push('\n');
    }
    out
}

/// #611: a record field as declared — its type and any `where` refinement —
/// attributed to the record that owns it. Mirrors how [`describe_type`] renders
/// the same field within the record body.
fn describe_record_field(t: &TypeDecl, f: &RecordField) -> String {
    let mut sig = format!("{}: {}", f.name.name, type_ref_str(&f.type_ref));
    if let Some(r) = &f.refinement {
        sig.push_str(&format!(" where {}", bynk_fmt::refinement_to_string(r)));
    }
    format!("```bynk\n{sig}\n```\n\nA field of `{}`.", t.name.name)
}

fn describe_type(t: &TypeDecl) -> String {
    let mut out = String::new();
    out.push_str("```bynk\n");
    // v0.157 (ADR 0183): render `[A, B]` type parameters on a generic type.
    let params = if t.type_params.is_empty() {
        String::new()
    } else {
        let names: Vec<&str> = t
            .type_params
            .iter()
            .map(|tp| tp.name.name.as_str())
            .collect();
        format!("[{}]", names.join(", "))
    };
    out.push_str(&format!("type {}{} = ", t.name.name, params));
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
        ServiceProtocol::WebSocket { .. } => " from websocket".to_string(),
    }
}

/// v0.131: a one-line summary of a `cors { }` policy for hover — the origins
/// (always present), then `credentials`/`maxAge` when set.
fn cors_summary(cors: &CorsPolicy) -> String {
    let mut parts = vec![format!("origins: {:?}", cors.origins())];
    if cors.credentials() {
        parts.push("credentials: true".to_string());
    }
    if let Some(secs) = cors.max_age_secs() {
        parts.push(format!("maxAge: {secs}s"));
    }
    parts.join(", ")
}

/// v0.141: a one-line summary of a `security { }` policy for hover — `nosniff`
/// (default on, shown when off) and `hsts` (when opted in).
fn security_summary(security: &SecurityPolicy) -> String {
    let mut parts = Vec::new();
    if !security.nosniff() {
        parts.push("nosniff: false".to_string());
    }
    if let Some(secs) = security.hsts_max_age_secs() {
        parts.push(format!("hsts: {secs}s"));
    }
    if parts.is_empty() {
        // The default posture (nosniff on, no HSTS) with an empty block.
        "nosniff".to_string()
    } else {
        parts.join(", ")
    }
}

/// v0.142 (ADR 0165): a one-line summary of a `limits { }` policy for hover — the
/// `maxBody` byte ceiling when set.
fn limits_summary(limits: &LimitsPolicy) -> String {
    match limits.max_body() {
        Some(bytes) => format!("maxBody: {bytes} bytes"),
        None => "maxBody".to_string(),
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
    // v0.131: the CORS policy, if any, renders as a `cors { … }` header line
    // summarising the origins (the load-bearing field).
    if let Some(cors) = &s.cors {
        out.push_str(&format!("\tcors {{ {} }}\n", cors_summary(cors)));
    }
    // v0.141: the security-headers policy, if declared, renders similarly.
    if let Some(security) = &s.security {
        out.push_str(&format!(
            "\tsecurity {{ {} }}\n",
            security_summary(security)
        ));
    }
    // v0.142 (ADR 0165): the request-limits policy, if declared, renders similarly.
    if let Some(limits) = &s.limits {
        out.push_str(&format!("\tlimits {{ {} }}\n", limits_summary(limits)));
    }
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
        // v0.157 (ADR 0183): a user generic-type application, as written.
        TypeRef::App { name, args, .. } => format!(
            "{}[{}]",
            name.name,
            args.iter().map(type_ref_str).collect::<Vec<_>>().join(", ")
        ),
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

    #[test]
    fn unit_reference_spans_links_the_suite_target() {
        // #609: the `suite <target>` header links to the unit under test, and any
        // `uses` clauses the fragment brings in link like the other unit kinds.
        let src = "suite todos\n  uses billing.charge\n";
        let spans = unit_reference_spans(src);
        let names: Vec<&str> = spans.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"todos"), "{names:?}");
        assert!(names.contains(&"billing.charge"), "{names:?}");
        // The span covers exactly the target name (so the link underlines it).
        let (_, span) = spans.iter().find(|(n, _)| n == "todos").unwrap();
        assert_eq!(&src[span.start..span.end], "todos");
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

    // v0.137.0 (ADR 0161): hover for the `key`/`store` contextual keywords and
    // the agent state fields they introduce — on the keyword or on the field
    // name alike, with a `store` field's annotations rendered.
    #[test]
    fn describe_agent_state_covers_key_store_keywords_and_fields() {
        let src = "context demo.app\n\nagent Sessions {\n  key id: String\n  store items: Map[String, Int] @indexed( by: id ) @bounded( 10000 )\n  on call read() -> Effect[Int] {\n    Effect.pure(0)\n  }\n}\n";

        // The `key` keyword and the key field name both render `key id: String`
        // plus the contextual-keyword doc.
        let at_key_kw = src.find("key id").unwrap();
        let key_kw = describe_agent_state_at(src, at_key_kw).expect("hover on `key`");
        assert!(key_kw.contains("key id: String"), "{key_kw}");
        assert!(key_kw.contains("identity field"), "doc line: {key_kw}");
        let at_key_name = src.find("id: String").unwrap();
        let key_name = describe_agent_state_at(src, at_key_name).expect("hover on the key field");
        assert_eq!(key_kw, key_name, "keyword and name hover match");

        // The `store` keyword and the store field name both render the field
        // signature — kind and annotations — plus the doc.
        let at_store_kw = src.find("store items").unwrap();
        let store_kw = describe_agent_state_at(src, at_store_kw).expect("hover on `store`");
        assert!(
            store_kw.contains("store items: Map[String, Int]"),
            "{store_kw}"
        );
        assert!(
            store_kw.contains("@indexed(by: id)"),
            "annotation: {store_kw}"
        );
        assert!(
            store_kw.contains("@bounded(10000)"),
            "annotation: {store_kw}"
        );
        assert!(
            store_kw.contains("persisted agent-state"),
            "doc line: {store_kw}"
        );
        let at_store_name = src.find("items:").unwrap();
        let store_name =
            describe_agent_state_at(src, at_store_name).expect("hover on the store field");
        assert_eq!(store_kw, store_name, "keyword and name hover match");

        // Not on a `key`/`store` keyword or state-field name → no hover (the
        // agent name, and the store kind, both fall through to other paths).
        assert!(describe_agent_state_at(src, src.find("Sessions").unwrap()).is_none());
        assert!(describe_agent_state_at(src, src.find("Map").unwrap()).is_none());
        // The word `id` inside `by: id` is an annotation argument, not the key
        // field's declaration — it must not masquerade as the key field.
        assert!(describe_agent_state_at(src, src.find("by: id").unwrap() + 4).is_none());
    }

    /// #611 (gap A): the reference half of the test above. Hover on a `key`/
    /// `store` field *use* inside the agent body — the case that resolved
    /// nowhere: state fields are absent from the project index and are not
    /// `let`/param locals, so every earlier hover path misses them.
    const TODOS: &str = "context demo.todos\n\
        \n\
        agent Todos {\n\
        \x20 key owner: String\n\
        \n\
        \x20 store items:   Map[String, Int]\n\
        \x20 store lastSeq: Cell[Int]\n\
        \n\
        \x20 invariant nonneg: lastSeq >= 0\n\
        \n\
        \x20 on call add(n: Int) -> Effect[()] {\n\
        \x20   let next = lastSeq + 1\n\
        \x20   let _ <- items.put(owner, next)\n\
        \x20   lastSeq := next\n\
        \x20   Effect.pure(())\n\
        \x20 }\n\
        }\n";

    #[test]
    fn describe_agent_state_covers_references_in_handler_bodies() {
        let src = TODOS;
        let at = |needle: &str| src.find(needle).expect("needle is in the fixture");

        // Every reference renders exactly what the declaration renders.
        let store_decl = describe_agent_state_at(src, at("store lastSeq")).unwrap();
        assert!(
            store_decl.contains("store lastSeq: Cell[Int]"),
            "{store_decl}"
        );
        for (what, needle) in [
            ("a bare read", "lastSeq + 1"),
            ("a `:=` write target", "lastSeq := next"),
            ("an invariant subject", "lastSeq >= 0"),
        ] {
            let hover = describe_agent_state_at(src, at(needle))
                .unwrap_or_else(|| panic!("no hover on {what}"));
            assert_eq!(hover, store_decl, "{what} hovers as its declaration");
        }
        // A store op's receiver — the `items` half of `items.put(…)`.
        let recv = describe_agent_state_at(src, at("items.put")).expect("hover on the receiver");
        assert_eq!(
            recv,
            describe_agent_state_at(src, at("store items")).unwrap()
        );

        // The `key` field is referenceable the same way.
        let key_decl = describe_agent_state_at(src, at("key owner")).unwrap();
        let key_ref = describe_agent_state_at(src, at("owner, next")).expect("hover on `owner`");
        assert_eq!(key_ref, key_decl);
        assert!(key_ref.contains("key owner: String"), "{key_ref}");

        // A name that is not state, and a state-shaped name outside the agent's
        // reference scope, both fall through to the other hover paths.
        assert!(describe_agent_state_at(src, at("next = lastSeq")).is_none());
        assert!(describe_agent_state_at(src, at("Effect.pure")).is_none());
    }

    /// #611 (gap C): hover on a `store` field's operation renders the registry
    /// signature over the field's declared kind.
    #[test]
    fn describe_store_op_renders_the_operation_signature() {
        let src = TODOS;
        let at_put = src
            .find("put(owner")
            .expect("the store op is in the fixture");
        let put = describe_store_op_at(src, at_put, &[]).expect("hover on `items.put`");
        assert!(put.contains("put(key: K, value: V) -> Effect[()]"), "{put}");
        // The field's declared kind grounds `K`/`V`, so it rides along.
        assert!(put.contains("store items: Map[String, Int]"), "{put}");

        // A local shadowing the receiver makes this an ordinary value method,
        // not a store op — mirroring the checker's by-provenance dispatch.
        let shadow = [bynk_check::locals::LocalBinding {
            name: "items".into(),
            def_span: Span { start: 0, end: 5 },
            kind: bynk_check::locals::LocalKind::Let,
            ty: "Map[String, Int]".into(),
            scope: Span {
                start: 0,
                end: src.len(),
            },
        }];
        assert!(describe_store_op_at(src, at_put, &shadow).is_none());

        // An operation the kind does not have, a receiver that is not a store
        // field, and a non-member identifier all fall through.
        let cell_put = src.replace("items.put(owner, next)", "lastSeq.put(owner, next)");
        assert!(
            describe_store_op_at(&cell_put, cell_put.find("put(owner").unwrap(), &[]).is_none(),
            "a `Cell` has no `put`"
        );
        assert!(describe_store_op_at(src, src.find("pure(()").unwrap(), &[]).is_none());
        assert!(describe_store_op_at(src, src.find("next = lastSeq").unwrap(), &[]).is_none());
    }

    /// A store op binds a **bare** ident receiver. A *qualified* receiver
    /// (`p.items.put(…)`) is an ordinary value method on a record field that
    /// merely shares a store field's name — the same class of confidently-wrong
    /// hover as gap B, and invisible to the index (which does not cover value
    /// methods), so nothing upstream would catch it.
    #[test]
    fn a_qualified_receiver_is_not_a_store_op_or_a_state_reference() {
        let src = "context demo.todos\n\
            \n\
            type Inner = { put: Int }\n\
            type Payload = { items: Inner, lastSeq: Int }\n\
            \n\
            agent Todos {\n\
            \x20 key owner: String\n\
            \x20 store items:   Map[String, Int]\n\
            \x20 store lastSeq: Cell[Int]\n\
            \n\
            \x20 on call add(p: Payload) -> Effect[()] {\n\
            \x20   let a = p.items.put\n\
            \x20   let b = p.lastSeq\n\
            \x20   Effect.pure(())\n\
            \x20 }\n\
            }\n";
        // `p.items.put` — the `put` is a field of `Inner`, not the store op.
        let at_put = src.find("put\n").expect("the qualified member");
        assert!(describe_store_op_at(src, at_put, &[]).is_none());
        // …and the `items` in `p.items` is a field of `Payload`, not the store
        // field — the same root cause, in the state-reference pass.
        let at_items = src.find("p.items").unwrap() + "p.".len();
        assert!(describe_agent_state_at(src, at_items).is_none());
        let at_seq = src.find("p.lastSeq").unwrap() + "p.".len();
        assert!(describe_agent_state_at(src, at_seq).is_none());

        // The bare forms in the same body still resolve.
        let bare = src.find("store items").unwrap();
        assert!(describe_agent_state_at(src, bare).is_some());
    }

    /// #611 (gap B): the index resolves a record-construction field label / field
    /// access to a `Field` key (`"Stored.title"`); hover must render it rather
    /// than fall through to the locals path, which name-matches in scope and
    /// bound `title:` to a same-named handler param.
    #[test]
    fn describe_symbol_renders_a_resolved_record_field() {
        let src = "context demo.todos\n\n\
            type Title = String where NonEmpty\n\n\
            type Stored = {\n\
            \x20 seq:   Int where NonNegative,\n\
            \x20 title: Title,\n\
            }\n";
        let title = describe_symbol(src, "Stored.title").expect("hover on `Stored.title`");
        assert!(title.contains("title: Title"), "{title}");
        assert!(title.contains("A field of `Stored`"), "{title}");
        // A field refinement rides along, as it does in the record body.
        let seq = describe_symbol(src, "Stored.seq").expect("hover on `Stored.seq`");
        assert!(seq.contains("seq: Int where NonNegative"), "{seq}");

        // The bare type name still renders the type, not a field.
        assert!(
            describe_symbol(src, "Stored")
                .unwrap()
                .contains("type Stored")
        );
        // An unknown field, an unknown owner, and a non-record owner yield none.
        assert!(describe_symbol(src, "Stored.nope").is_none());
        assert!(describe_symbol(src, "Nope.title").is_none());
        assert!(describe_symbol(src, "Title.title").is_none());
    }

    /// v0.166 (#616): the actor arm — both declaration forms. The reference-offset
    /// fixture in `hover_references.rs` covers the `Bearer` form against real
    /// analysis output; the schemes without config and ADR 0091's refinement form
    /// are declared by no example project, so they are pinned here.
    #[test]
    fn describe_symbol_renders_an_actor_in_both_forms() {
        let src = "context demo.auth\n\n\
            type UserId = String where NonEmpty\n\n\
            ---\n\
            A signed-in user.\n\
            ---\n\
            actor User { auth = Bearer(secret = \"AUTH_JWT_SECRET\"), identity = UserId }\n\n\
            actor Public { auth = None }\n\n\
            actor Worker { auth = Internal }\n\n\
            actor Admin = User where hasClaim(\"admin\")\n";

        let user = describe_symbol(src, "User").expect("hover on `User`");
        assert!(
            user.contains(
                "actor User { auth = Bearer(secret = \"AUTH_JWT_SECRET\"), identity = UserId }"
            ),
            "{user}"
        );
        // The doc block rides along, as it does for every other declaration.
        assert!(user.contains("A signed-in user."), "{user}");

        // A scheme with no config and no identity renders neither.
        let public = describe_symbol(src, "Public").expect("hover on `Public`");
        assert!(
            public.contains("actor Public { auth = None }") && !public.contains("identity"),
            "{public}"
        );
        assert!(
            describe_symbol(src, "Worker")
                .unwrap()
                .contains("actor Worker { auth = Internal }")
        );

        // ADR 0091's refinement form renders its base and claim predicate.
        let admin = describe_symbol(src, "Admin").expect("hover on `Admin`");
        assert!(
            admin.contains("actor Admin = User where hasClaim(\"admin\")"),
            "{admin}"
        );

        assert!(describe_symbol(src, "Nobody").is_none());
    }

    /// v0.166 (#616, review): the actor arm claims to mirror `bynk-fmt`'s
    /// `format_actor`, so it must escape a scheme-config string the same way.
    /// `SchemeArgValue::Str` holds the value *unescaped* — the parser resolves
    /// `\"`/`\\`/`\n`/`\t` at lex time — so rendering it raw put invalid Bynk
    /// inside a ```bynk fence. Pinned against the formatter's own output rather
    /// than a hand-written expectation: a copy would agree only until one moved.
    #[test]
    fn describe_symbol_escapes_an_actors_scheme_config() {
        let src = "context demo.auth\n\n\
            actor User { auth = Bearer(secret = \"a\\\"b\\\\c\") }\n";
        let hover = describe_symbol(src, "User").expect("hover on `User`");
        assert!(
            hover.contains("actor User { auth = Bearer(secret = \"a\\\"b\\\\c\") }"),
            "the config value must round-trip escaped:\n{hover}"
        );

        // The fenced declaration is exactly what the formatter emits for it.
        let formatted = bynk_fmt::format_source(src, &bynk_fmt::FormatOptions::default())
            .expect("the fixture formats");
        let actor_line = formatted
            .lines()
            .find(|l| l.starts_with("actor User"))
            .expect("the actor line");
        assert!(
            hover.contains(actor_line),
            "hover:\n{hover}\nfmt: {actor_line}"
        );
    }

    /// v0.166 (#616): the capability-op arm, keyed `"Cap.op"` (ADR 0069) —
    /// attributed to its owner, as a field is to its record.
    #[test]
    fn describe_symbol_renders_a_capability_operation() {
        let src = "context demo.svc\n\n\
            capability Logger {\n\
            \x20 ---\n\
            \x20 Record a line.\n\
            \x20 ---\n\
            \x20 fn info(message: String) -> Effect[()]\n\
            }\n\n\
            capability Clock {\n\
            \x20 fn now() -> Effect[Int]\n\
            }\n";

        let info = describe_symbol(src, "Logger.info").expect("hover on `Logger.info`");
        assert!(
            info.contains("fn info(message: String) -> Effect[()]"),
            "{info}"
        );
        assert!(
            info.contains("An operation of capability `Logger`"),
            "{info}"
        );
        assert!(info.contains("Record a line."), "{info}");

        // A no-arg op on another capability — the owner is what disambiguates.
        let now = describe_symbol(src, "Clock.now").expect("hover on `Clock.now`");
        assert!(now.contains("fn now() -> Effect[Int]"), "{now}");

        // The bare capability name still renders the capability itself.
        assert!(
            describe_symbol(src, "Logger")
                .unwrap()
                .contains("capability Logger")
        );
        // An unknown op, and an op read against the wrong owner, yield none.
        assert!(describe_symbol(src, "Logger.nope").is_none());
        assert!(describe_symbol(src, "Clock.info").is_none());
    }

    /// v0.166 (#616, ADR 0191 D2): a bare key names a *free* function. Matching a
    /// method on its bare name answered with whichever type declared it first —
    /// `Gauge.bump`'s own declaration rendered `Counter.bump` — and silently
    /// outranked the index's real answer.
    #[test]
    fn describe_symbol_keys_methods_by_their_compound_name_only() {
        let src = "context demo.shop\n\n\
            type Counter = { count: Int }\n\
            type Gauge = { level: Int }\n\n\
            fn Counter.bump(self) -> Counter { Counter { count: self.count + 1 } }\n\n\
            fn Gauge.bump(self) -> Gauge { Gauge { level: self.level + 1 } }\n\n\
            fn free(n: Int) -> Int { n }\n";

        // The type prefix is the identity: each compound key renders its own.
        let counter = describe_symbol(src, "Counter.bump").expect("hover on `Counter.bump`");
        assert!(
            counter.contains("fn Counter.bump(self) -> Counter") && !counter.contains("Gauge"),
            "{counter}"
        );
        let gauge = describe_symbol(src, "Gauge.bump").expect("hover on `Gauge.bump`");
        assert!(
            gauge.contains("fn Gauge.bump(self) -> Gauge") && !gauge.contains("Counter"),
            "{gauge}"
        );

        // A bare `bump` names no method: it is a guess between the two, and the
        // index's compound key is what resolves them.
        assert!(describe_symbol(src, "bump").is_none());
        // A free function is still keyed by its bare name.
        assert!(
            describe_symbol(src, "free")
                .unwrap()
                .contains("fn free(n: Int) -> Int")
        );
        assert!(describe_symbol(src, "Counter.nope").is_none());
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
                args: Vec::new(),
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
                args: Vec::new(),
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
                        args: Vec::new(),
                    },
                )]
            )
            .is_none()
        );
    }

    const CACHE_SVC: &str = "context api\nservice api from http {\n  @cache(maxAge: 5.minutes, scope: public)\n  on GET(\"/x\") () -> Effect[HttpResult[String]] by v: Visitor {\n    Ok(\"y\")\n  }\n}\n";

    #[test]
    fn hover_on_cache_annotation_describes_it() {
        // Offset on the `cache` name token.
        let offset = CACHE_SVC.find("cache").unwrap() + 1;
        let hover = describe_handler_annotation_at(CACHE_SVC, offset).expect("hovers @cache");
        assert!(hover.contains("`@cache`"), "names the annotation: {hover}");
        assert!(hover.contains("maxAge"), "documents maxAge: {hover}");
        assert!(hover.contains("scope"), "documents scope: {hover}");
        // Off the annotation (on the `Ok` body) — no annotation hover.
        let ok_offset = CACHE_SVC.find("Ok(").unwrap() + 1;
        assert!(describe_handler_annotation_at(CACHE_SVC, ok_offset).is_none());
    }

    #[test]
    fn annotation_token_spans_cover_name_and_labels() {
        let spans = handler_annotation_token_spans(CACHE_SVC);
        // `@cache` + the two argument labels `maxAge`/`scope`.
        assert_eq!(spans.len(), 3, "{spans:?}");
        let texts: Vec<&str> = spans.iter().map(|s| &CACHE_SVC[s.start..s.end]).collect();
        assert_eq!(texts, ["@cache", "maxAge", "scope"]);
        // A service with no annotations yields nothing.
        let plain = "context api\nservice api from http {\n  on GET(\"/x\") () -> Effect[HttpResult[String]] by v: Visitor { Ok(\"y\") }\n}\n";
        assert!(handler_annotation_token_spans(plain).is_empty());
    }
}
