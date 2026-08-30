//! P8.1 (#1512): `UnitId` and `UnitSignature` — the load-bearing types this
//! whole phase's firewall (R3.14) is stated in terms of. `UnitSignature`
//! projects a [`UnitTable`] into design notes §15's four required-annotation
//! categories — cross-context types, fn signatures, handler signatures plus
//! storage, capability sets — with every body/body-adjacent field excluded
//! ([DECISION B], `design/tracks/incrementality.md` §3.4/Q4) rather than
//! merely ignored by the comparison: `body`/`requires`/`ensures` never reach
//! [`FnSignature`], `body` never reaches [`HandlerSignature`], `init`/
//! `annotations` never reach [`StoreFieldSignature`] — the type system, not a
//! comparison function's own discipline, makes "no body reachable from
//! `UnitSignature`" a fact.
//!
//! Stability is proved by comparing [`UnitSignature::canonical`], not the raw
//! struct: every included fragment still carries its own `Span` (`Ident`,
//! `Param`, every `TypeRef` variant, `TypeDecl`'s own `trivia`/
//! `documentation`), and editing a body shifts every later declaration's own
//! spans in the same file (the byte-offset cascade PR #1509's bot review
//! caught one level up, in `UnitSignature`'s own design). [DECISION C]
//! extends `contract.rs`'s `canon_type`/`service_normal_form` (ADR 0200) —
//! already a span-free canonical rendering, proven correct by
//! `contract_hash.rs`'s own no-false-positive fixture — to reach the new
//! signature shapes here, rather than inventing a second erasure scheme.
//!
//! Nothing in the tree calls [`unit_signature_for`] yet, per this slice's own
//! "Gated on: —" row in `design/tracks/incrementality.md` §6: P8.2 (the
//! stability fixture) is the first caller, P8.3/P8.5 the first structural
//! consumers.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use bynk_syntax::ast::{CapRef, Param, TypeDecl, TypeRef};

use crate::symbols::UnitTable;

/// [DECISION A]: a stable, hashable identity for a unit, reusing the
/// unit-name `String` every `UnitTable`/`combined_types_for` caller already
/// keys on today, rather than introducing a fresh interned-integer scheme —
/// no `index_vec` crate (or hand-rolled equivalent) exists anywhere in this
/// codebase, confirmed by grep. `UnitSignature` only needs `UnitId` to be a
/// stable, hashable, comparable identity for R3.14's own proof; it does not
/// need to be dense or `IndexVec`-compatible. P8.3 is the slice that decides
/// whether this widens to a dense integer index once `ProjectGraph`'s own
/// `units: IndexVec<UnitId, Unit>` shape is built, or whether `ProjectGraph`
/// adapts to a string-keyed `UnitId` instead — left open here on purpose.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UnitId(pub String);

/// [DECISION B]: [`Param`], body-free (it already is — kept as its own type
/// only so [`FnSignature`]/[`HandlerSignature`] don't reach back into
/// `bynk-syntax::ast::Param` and risk a future field being added there and
/// silently flowing through unexamined).
#[derive(Debug, Clone)]
pub struct ParamSignature {
    pub name: String,
    pub type_ref: TypeRef,
}

fn param_signatures(params: &[Param]) -> Vec<ParamSignature> {
    params
        .iter()
        .map(|p| ParamSignature {
            name: p.name.name.clone(),
            type_ref: p.type_ref.clone(),
        })
        .collect()
}

/// [DECISION B]: [`bynk_syntax::ast::FnDecl`], body-free. Excludes `body`,
/// `requires`, `ensures` (all body-adjacent — `requires`/`ensures` scope over
/// parameters and the result but are not part of design notes §15's own
/// required-annotation list, per Q4) and `documentation`.
#[derive(Debug, Clone)]
pub struct FnSignature {
    pub type_params: Vec<String>,
    pub name: String,
    pub params: Vec<ParamSignature>,
    pub return_type: TypeRef,
    pub has_self: bool,
}

/// [DECISION B]: [`bynk_syntax::ast::Handler`], body-free. Excludes `body`,
/// `by_clause` (an actor binder, resolved at the call boundary rather than
/// part of the wire-visible shape) and `documentation`. `given` is kept —
/// it's this handler's own slice of the capability-set category.
#[derive(Debug, Clone)]
pub struct HandlerSignature {
    pub method_name: Option<String>,
    pub params: Vec<ParamSignature>,
    pub return_type: TypeRef,
    pub given: Vec<String>,
}

/// [DECISION B]: [`bynk_syntax::ast::StoreField`], body-free. Excludes `init`
/// and `annotations` (both body-adjacent per Q4 — annotations govern the
/// field's own storage behaviour, not its externally-relevant shape) and
/// `documentation`.
#[derive(Debug, Clone)]
pub struct StoreFieldSignature {
    pub name: String,
    pub kind_head: String,
    pub kind_args: Vec<TypeRef>,
}

/// The capability-set category (design notes §15's fourth): what a unit
/// exports (`table.exported_capabilities`, already plain strings) plus what
/// each provider/service declares it needs (`given`/`default_given`). Every
/// `CapRef` collapses to its rendered name (`context.capability`, or bare
/// `capability` when local) — its own `Span` is dropped, matching every
/// other category's span-erasure discipline.
#[derive(Debug, Clone, Default)]
pub struct CapabilitySignature {
    pub exported: BTreeSet<String>,
    /// Keyed by the implemented capability's own name (`ProviderDecl.capability`).
    pub provider_given: BTreeMap<String, Vec<String>>,
    /// Keyed by service name (`ServiceDecl.default_given`).
    pub service_given: BTreeMap<String, Vec<String>>,
}

/// A `CapRef`'s canonical name: `context.capability` when cross-context,
/// bare `capability` when local. Spans are dropped — matching every other
/// category's span-erasure discipline.
fn cap_ref_name(c: &CapRef) -> String {
    match &c.context {
        Some(q) => format!("{}.{}", q.joined(), c.key()),
        None => c.key().to_string(),
    }
}

fn cap_ref_names(refs: &[CapRef]) -> Vec<String> {
    refs.iter().map(cap_ref_name).collect()
}

/// P8.1 (#1512): a unit's stable signature — everything about it that must
/// survive an edit inside a function/handler body untouched (R3.14, the
/// phase's firewall).
///
/// Deliberately does not derive `PartialEq`: two signatures compare equal
/// through [`UnitSignature::canonical`] ([DECISION C]), not by structural
/// equality on the raw fields — the same "compare the canonical form, not
/// the raw AST value" shape `contract_hash.rs`'s own fixture already trusts
/// for ADR 0200. Comparing the raw struct would reintroduce exactly the
/// span-instability this type exists to erase.
#[derive(Debug, Clone)]
pub struct UnitSignature {
    pub id: UnitId,
    /// `combined_types_for`'s own output, reused unchanged ([DECISION A] of
    /// ADR 0412, Q1) — the cross-context-types category.
    pub combined_types: HashMap<String, Arc<TypeDecl>>,
    /// Keyed by `FnName`'s own rendered form (`f`, or `T.method`), matching
    /// `UnitTable.fns`'s own key.
    pub fns: BTreeMap<String, FnSignature>,
    /// Keyed by owning service/agent name; each maps to its handlers in
    /// declaration order (handlers have no name of their own to key by —
    /// `method_name` is only set for agent handlers).
    pub handlers: BTreeMap<String, Vec<HandlerSignature>>,
    /// Keyed by owning agent name; `UnitTable.services`/`.providers` carry no
    /// store fields of their own.
    pub store_fields: BTreeMap<String, Vec<StoreFieldSignature>>,
    pub capabilities: CapabilitySignature,
}

/// Builds a [`UnitSignature`] for `name` from `table`, reusing `combined_types`
/// (typically a fresh `combined_types_for(name, ..)` call, mirrored here as a
/// parameter rather than recomputed so a caller building both alongside each
/// other pays for one traversal, not two).
///
/// Every category below is a direct, field-by-field projection off
/// `UnitTable`'s own real shapes — see this module's own doc comment and
/// `design/tracks/incrementality.md` §3.4 (Q4) for which fields are excluded
/// and why.
pub fn unit_signature_for(
    name: &str,
    table: &UnitTable,
    combined_types: HashMap<String, Arc<TypeDecl>>,
) -> UnitSignature {
    let fns = table
        .fns
        .iter()
        .map(|(fname, decl)| {
            let sig = FnSignature {
                type_params: decl
                    .type_params
                    .iter()
                    .map(|tp| tp.name.name.clone())
                    .collect(),
                name: fname.clone(),
                params: param_signatures(&decl.params),
                return_type: decl.return_type.clone(),
                has_self: decl.has_self,
            };
            (fname.clone(), sig)
        })
        .collect();

    let mut handlers: BTreeMap<String, Vec<HandlerSignature>> = BTreeMap::new();
    for (sname, sdecl) in &table.services {
        handlers.insert(
            sname.clone(),
            sdecl.handlers.iter().map(handler_signature).collect(),
        );
    }

    let mut store_fields: BTreeMap<String, Vec<StoreFieldSignature>> = BTreeMap::new();
    for (aname, adecl) in &table.agents {
        handlers.insert(
            aname.clone(),
            adecl.handlers.iter().map(handler_signature).collect(),
        );
        store_fields.insert(
            aname.clone(),
            adecl
                .store_fields
                .iter()
                .map(|f| StoreFieldSignature {
                    name: f.name.name.clone(),
                    kind_head: f.kind.head.name.clone(),
                    kind_args: f.kind.args.clone(),
                })
                .collect(),
        );
    }

    let mut provider_given: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for pdecl in table.providers.values() {
        provider_given.insert(pdecl.capability.name.clone(), cap_ref_names(&pdecl.given));
    }

    let mut service_given: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (sname, sdecl) in &table.services {
        service_given.insert(sname.clone(), cap_ref_names(&sdecl.default_given));
    }

    UnitSignature {
        id: UnitId(name.to_string()),
        combined_types,
        fns,
        handlers,
        store_fields,
        capabilities: CapabilitySignature {
            exported: table.exported_capabilities.iter().cloned().collect(),
            provider_given,
            service_given,
        },
    }
}

fn handler_signature(h: &bynk_syntax::ast::Handler) -> HandlerSignature {
    HandlerSignature {
        method_name: h.method_name.as_ref().map(|i| i.name.clone()),
        params: param_signatures(&h.params),
        return_type: h.return_type.clone(),
        given: cap_ref_names(&h.given),
    }
}

impl UnitSignature {
    /// [DECISION C]: the canonical, span-free rendering R3.14's own proof
    /// compares (`unit_signature_before.canonical() ==
    /// unit_signature_after.canonical()`), never the raw struct. Delegates
    /// every fragment through `contract.rs`'s canonical renderers so this
    /// type and `service_contract_hash` cannot silently diverge on what
    /// "the same type" means.
    pub fn canonical(&self) -> String {
        crate::contract::canon_unit_signature(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use bynk_project::{ParsedFile, UnitKind};
    use bynk_syntax::ast::SourceUnit;

    use crate::symbols::{build_unit_table, combined_types_for};

    fn parsed(source_path: &str, src: &str) -> ParsedFile {
        let tokens = bynk_syntax::lexer::tokenize(src).expect("lex");
        let unit = bynk_syntax::parser::parse_unit(&tokens, src).expect("parse");
        let kind = match &unit {
            SourceUnit::Commons(_) => UnitKind::Commons,
            SourceUnit::Context(_) => UnitKind::Context,
            SourceUnit::Suite(_) | SourceUnit::Adapter(_) => unreachable!("fixture only"),
        };
        ParsedFile::new(
            PathBuf::from(source_path),
            PathBuf::from(source_path),
            None,
            src.to_string(),
            unit,
            kind,
            false,
        )
    }

    const SHARED_SRC: &str = r#"commons demo.shared

type Widget = {
  id: String,
}
"#;

    const CONTEXT_SRC: &str = r#"context demo.svc

uses demo.shared

exports capability { Notifier }

fn double(x: Int) -> Int {
  x * 2
}

capability Notifier {
  fn ping(seed: Int) -> Effect[Int]
}

provides Notifier = StubNotifier {
  fn ping(seed: Int) -> Effect[Int] {
    Effect.pure(seed + 1)
  }
}

agent Counter {
  key id: String

  store count: Cell[Int]

  on call increment() -> Effect[Int] {
    count := count + 1
    Effect.pure(count)
  }
}

service api from http {
  on GET("/widgets/:id") (id: String) -> Effect[HttpResult[Widget]] by Visitor given Notifier {
    Ok(Widget { id: id })
  }
}
"#;

    /// P8.1 (#1512), the issue's own "Done when": builds a `UnitSignature`
    /// for a fixture unit carrying a function (`double`), a handler in both
    /// a service (`api`'s `GET` route) and an agent (`Counter`'s
    /// `increment`), a `store` field (`Counter.count`) and a cross-context
    /// type reference (`Widget`, reached via `uses demo.shared`) — every
    /// category [DECISION B]'s field list names — and proves the
    /// field-exclusion list is complete for all of them.
    ///
    /// The exclusion itself is a **compile-time fact, not a runtime
    /// assertion**, per [DECISION B]: `FnSignature`/`HandlerSignature`/
    /// `StoreFieldSignature` have no `body`/`requires`/`ensures`/`init`/
    /// `annotations` field to hold one — this test's real job is proving the
    /// builder actually reaches every category on real parsed source (a
    /// hand-assembled AST fixture could hide a field the builder silently
    /// skips), and asserting the shape that *does* survive is exactly the
    /// externally-relevant one.
    #[test]
    fn unit_signature_excludes_every_body_and_body_adjacent_field() {
        let shared = parsed("demo/shared.bynk", SHARED_SRC);
        let context = parsed("demo/svc.bynk", CONTEXT_SRC);

        let mut errors = Vec::new();
        let shared_table = build_unit_table(
            "demo.shared",
            UnitKind::Commons,
            &[0],
            &[shared],
            &mut errors,
        );
        let context_table =
            build_unit_table("demo.svc", UnitKind::Context, &[0], &[context], &mut errors);
        assert!(errors.is_empty(), "fixture must parse cleanly: {errors:?}");

        let mut unit_tables = HashMap::new();
        unit_tables.insert("demo.shared".to_string(), shared_table);
        unit_tables.insert("demo.svc".to_string(), context_table.clone());
        let mut unit_uses = HashMap::new();
        unit_uses.insert("demo.svc".to_string(), vec!["demo.shared".to_string()]);

        let combined = combined_types_for("demo.svc", &unit_tables, &unit_uses);
        let sig = unit_signature_for("demo.svc", &context_table, combined);

        assert_eq!(sig.id, UnitId("demo.svc".to_string()));

        // Cross-context type reference: `Widget` reached only through `uses`.
        assert!(sig.combined_types.contains_key("Widget"));

        // Fn signature.
        let f = sig.fns.get("double").expect("double must be projected");
        assert_eq!(f.params.len(), 1);
        assert_eq!(f.params[0].name, "x");

        // Service handler, including its own `given`.
        let api_handlers = sig
            .handlers
            .get("api")
            .expect("api's handler must be projected");
        assert_eq!(api_handlers.len(), 1);
        assert_eq!(api_handlers[0].given, vec!["Notifier".to_string()]);

        // Agent handler and store field.
        let agent_handlers = sig
            .handlers
            .get("Counter")
            .expect("Counter's handler must be projected");
        assert_eq!(agent_handlers.len(), 1);
        assert_eq!(agent_handlers[0].method_name.as_deref(), Some("increment"));

        let store = sig
            .store_fields
            .get("Counter")
            .expect("store field must be projected");
        assert_eq!(store.len(), 1);
        assert_eq!(store[0].name, "count");
        assert_eq!(store[0].kind_head, "Cell");

        // Capability-set category: what's exported and what a provider needs.
        assert!(sig.capabilities.exported.contains("Notifier"));
        assert_eq!(
            sig.capabilities.provider_given.get("Notifier"),
            Some(&Vec::<String>::new())
        );

        // The canonical rendering must actually run over every category above
        // without panicking, and must be deterministic.
        let rendered = sig.canonical();
        assert_eq!(rendered, sig.canonical());
        assert!(rendered.contains("double"));
        assert!(rendered.contains("Widget"));
        assert!(rendered.contains("count"));
        assert!(rendered.contains("Notifier"));
    }
}
