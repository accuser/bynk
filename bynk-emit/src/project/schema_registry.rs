//! Events track, slice 3c (#980): the cross-build schema registry.
//!
//! Slice 3b (#978) added `@schema(N)` as an unconditionally-trusted
//! assertion — nothing verified it, because nothing existed to verify it
//! against. This module is that verification, plus the auto-bump path for
//! events that don't annotate at all: a committed `bynk.schema.lock`, keyed
//! by qualified event name, storing each event's field shape (name, type,
//! whether the field has a default) and its current schema version. Every
//! build reconciles the live shape against the stored one and either leaves
//! the version alone, bumps it (a purely additive change), or fails the
//! build (a change that isn't safely additive).
//!
//! Read/write is opt-in (`CompileOptions::schema_registry`) — see
//! `project.rs`'s `compile_project`/`run_checks` for why: fixtures and
//! in-memory builds must never touch a project tree as a side effect of
//! compiling.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use bynk_syntax::ast::{EventDecl, TypeRef};
use bynk_syntax::error::CompileError;

use super::UnitTable;

const LOCK_FILE: &str = "bynk.schema.lock";

/// Where the registry lives for a given project root — exposed so callers
/// reporting a write failure can name the actual file, not just the root.
pub(crate) fn lock_path(project_root: &Path) -> std::path::PathBuf {
    project_root.join(LOCK_FILE)
}

fn lock_version() -> u32 {
    1
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SchemaRegistry {
    // No serde `default`: a registry with no `version` is not a fresh
    // project, it is corruption (a truncated write), and must fail the read
    // rather than silently re-baseline every event's history — the same
    // argument `bynk.deploy.lock`'s `DeployLock` makes for the identical
    // field.
    version: u32,
    #[serde(default)]
    events: BTreeMap<String, EventEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct EventEntry {
    schema: i64,
    fields: Vec<FieldShape>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct FieldShape {
    name: String,
    #[serde(rename = "type")]
    ty: String,
    default: bool,
}

/// A shallow, per-field snapshot of an event's current shape — deliberately
/// **not** `bynk-check/src/contract.rs`'s `canon_named_in`: that renders a
/// field as `name: type` with no signal for default-presence, so an additive
/// change (new field, has a default) and a breaking one (new field, no
/// default) perturb it identically. This snapshot exists to tell those two
/// apart, one field at a time.
fn snapshot(event: &EventDecl) -> Vec<FieldShape> {
    let mut fields: Vec<FieldShape> = event
        .body
        .fields
        .iter()
        .map(|f| FieldShape {
            name: f.name.name.clone(),
            ty: canon_type(&f.type_ref),
            default: f.init.is_some(),
        })
        .collect();
    fields.sort_by(|a, b| a.name.cmp(&b.name));
    fields
}

/// A stable surface-form rendering of a `TypeRef`, for the registry's own
/// stored shape only. Deliberately **not** `bynk-fmt`'s `type_ref_to_string`
/// (private, and `bynk-emit` does not depend on `bynk-fmt`) — and
/// deliberately not required to match either the formatter's or the
/// checker's own rendering byte-for-byte: this string is compared only
/// against itself, across builds, never displayed to a user. Exhaustive, no
/// wildcard arm, so a new `TypeRef` variant is a compile error here rather
/// than a silent "no signal" gap in the registry's diffing.
fn canon_type(t: &TypeRef) -> String {
    match t {
        TypeRef::Base(b, _) => b.name().to_string(),
        TypeRef::Named(id) => id.name.clone(),
        TypeRef::Result(a, b, _) => format!("Result[{}, {}]", canon_type(a), canon_type(b)),
        TypeRef::Option(t, _) => format!("Option[{}]", canon_type(t)),
        TypeRef::Effect(t, _) => format!("Effect[{}]", canon_type(t)),
        TypeRef::HttpResult(t, _) => format!("HttpResult[{}]", canon_type(t)),
        TypeRef::QueueResult(_) => "QueueResult".to_string(),
        TypeRef::List(t, _) => format!("List[{}]", canon_type(t)),
        TypeRef::Map(k, v, _) => format!("Map[{}, {}]", canon_type(k), canon_type(v)),
        TypeRef::Query(t, _) => format!("Query[{}]", canon_type(t)),
        TypeRef::Stream(t, _) => format!("Stream[{}]", canon_type(t)),
        TypeRef::Connection(t, _) => format!("Connection[{}]", canon_type(t)),
        TypeRef::History(t, _) => format!("History[{}]", canon_type(t)),
        TypeRef::ValidationError(_) => "ValidationError".to_string(),
        TypeRef::JsonError(_) => "JsonError".to_string(),
        TypeRef::Unit(_) => "()".to_string(),
        TypeRef::Fn(params, ret, _) => format!(
            "({}) -> {}",
            params.iter().map(canon_type).collect::<Vec<_>>().join(", "),
            canon_type(ret)
        ),
        TypeRef::App { name, args, .. } => format!(
            "{}[{}]",
            name.name,
            args.iter().map(canon_type).collect::<Vec<_>>().join(", ")
        ),
    }
}

/// The outcome of reconciling one event against its (possibly absent)
/// registry entry.
enum Reconciled {
    /// No prior entry: baseline silently at `declared` (the event's current
    /// `@schema(N)`, or `1`).
    Baseline { fields: Vec<FieldShape> },
    /// Shape unchanged since the stored entry.
    Unchanged {
        fields: Vec<FieldShape>,
        stored: i64,
    },
    /// Every added field carries a default; nothing removed or retyped.
    Additive {
        fields: Vec<FieldShape>,
        bumped: i64,
    },
    /// A field was removed, retyped, added without a default, or lost a
    /// default it previously had.
    NonAdditive {
        removed: Vec<String>,
        retyped: Vec<String>,
        added_without_default: Vec<String>,
        lost_default: Vec<String>,
    },
}

fn reconcile_one(current: &[FieldShape], stored: Option<&EventEntry>) -> Reconciled {
    let Some(stored) = stored else {
        return Reconciled::Baseline {
            fields: current.to_vec(),
        };
    };
    if current == stored.fields.as_slice() {
        return Reconciled::Unchanged {
            fields: current.to_vec(),
            stored: stored.schema,
        };
    }

    fn by_name(fields: &[FieldShape]) -> BTreeMap<&str, &FieldShape> {
        fields.iter().map(|f| (f.name.as_str(), f)).collect()
    }
    let old = by_name(&stored.fields);
    let new = by_name(current);

    let removed: Vec<String> = old
        .keys()
        .filter(|n| !new.contains_key(*n))
        .map(|n| n.to_string())
        .collect();
    let retyped: Vec<String> = old
        .iter()
        .filter_map(|(n, old_field)| {
            new.get(n)
                .filter(|new_field| new_field.ty != old_field.ty)
                .map(|_| n.to_string())
        })
        .collect();
    let added_without_default: Vec<String> = new
        .iter()
        .filter(|(n, f)| !old.contains_key(*n) && !f.default)
        .map(|(n, _)| n.to_string())
        .collect();
    // A field present in both, same type, that had a default and no longer
    // does: an older wire event that omitted this key (relying on the
    // default) can no longer decode — the same forward-compatibility break
    // as a field added without one, just approached from the other side.
    let lost_default: Vec<String> = old
        .iter()
        .filter_map(|(n, old_field)| {
            new.get(n).filter(|new_field| {
                new_field.ty == old_field.ty && old_field.default && !new_field.default
            })
        })
        .map(|f| f.name.clone())
        .collect();

    if removed.is_empty()
        && retyped.is_empty()
        && added_without_default.is_empty()
        && lost_default.is_empty()
    {
        Reconciled::Additive {
            fields: current.to_vec(),
            bumped: stored.schema + 1,
        }
    } else {
        Reconciled::NonAdditive {
            removed,
            retyped,
            lost_default,
            added_without_default,
        }
    }
}

/// Reconcile every event across every unit against the given registry.
/// Returns the updated registry document and the effective schema version
/// for each event, keyed the same way as the document itself
/// (`<unit>.<EventName>`). Diagnostics for a mismatched `@schema(N)` or a
/// non-additive change are pushed onto `errors`.
pub(crate) fn reconcile(
    existing: &SchemaRegistry,
    unit_tables: &std::collections::HashMap<String, UnitTable>,
    errors: &mut Vec<CompileError>,
) -> (SchemaRegistry, std::collections::HashMap<String, i64>) {
    let mut updated = SchemaRegistry {
        version: lock_version(),
        events: BTreeMap::new(),
    };
    let mut effective = std::collections::HashMap::new();

    let mut units: Vec<_> = unit_tables.iter().collect();
    units.sort_by_key(|(name, _)| *name);

    for (unit_name, table) in units {
        let mut events: Vec<_> = table.events.iter().collect();
        events.sort_by_key(|(name, _)| *name);
        for (event_name, event) in events {
            let key = format!("{unit_name}.{event_name}");
            let fields = snapshot(event);
            let declared = event.schema_version();
            let annotation_span = event
                .annotations
                .iter()
                .find(|a| a.name.name == "schema")
                .map(|a| a.span);
            let has_annotation = annotation_span.is_some();

            let (effective_version, entry) = match reconcile_one(&fields, existing.events.get(&key))
            {
                Reconciled::Baseline { fields } => (
                    declared,
                    EventEntry {
                        schema: declared,
                        fields,
                    },
                ),
                Reconciled::Unchanged { fields, stored } => {
                    if has_annotation && declared != stored {
                        errors.push(mismatch_error(
                            event_name,
                            annotation_span.unwrap(),
                            declared,
                            stored,
                        ));
                    }
                    (
                        stored,
                        EventEntry {
                            schema: stored,
                            fields,
                        },
                    )
                }
                Reconciled::Additive { fields, bumped } => {
                    if has_annotation && declared != bumped {
                        errors.push(mismatch_error(
                            event_name,
                            annotation_span.unwrap(),
                            declared,
                            bumped,
                        ));
                    }
                    (
                        bumped,
                        EventEntry {
                            schema: bumped,
                            fields,
                        },
                    )
                }
                Reconciled::NonAdditive {
                    removed,
                    retyped,
                    added_without_default,
                    lost_default,
                } => {
                    errors.push(non_additive_error(
                        event,
                        event_name,
                        &removed,
                        &retyped,
                        &added_without_default,
                        &lost_default,
                    ));
                    // Keep the old entry as-is: a build that fails here
                    // must not perturb the registry (`compile_project`
                    // skips the write on any error too — this just keeps
                    // the in-memory document consistent with that).
                    let old = existing.events[&key].clone();
                    (old.schema, old)
                }
            };

            effective.insert(key.clone(), effective_version);
            updated.events.insert(key, entry);
        }
    }

    (updated, effective)
}

fn mismatch_error(
    event_name: &str,
    span: bynk_syntax::span::Span,
    declared: i64,
    computed: i64,
) -> CompileError {
    CompileError::new(
        "bynk.event.schema_version_mismatch",
        span,
        format!(
            "`{event_name}`'s `@schema({declared})` disagrees with the schema \
             registry, which computes version {computed} from the event's \
             build history"
        ),
    )
    .with_note(format!(
        "update the annotation to `@schema({computed})`, or remove it to let \
         the compiler track the version automatically"
    ))
}

fn non_additive_error(
    event: &EventDecl,
    event_name: &str,
    removed: &[String],
    retyped: &[String],
    added_without_default: &[String],
    lost_default: &[String],
) -> CompileError {
    let mut parts = Vec::new();
    if !removed.is_empty() {
        parts.push(format!("field(s) removed: {}", removed.join(", ")));
    }
    if !retyped.is_empty() {
        parts.push(format!("field(s) retyped: {}", retyped.join(", ")));
    }
    if !added_without_default.is_empty() {
        parts.push(format!(
            "field(s) added without a default: {}",
            added_without_default.join(", ")
        ));
    }
    if !lost_default.is_empty() {
        parts.push(format!(
            "field(s) lost their default: {}",
            lost_default.join(", ")
        ));
    }
    CompileError::new(
        "bynk.event.non_additive_schema_change",
        event.span,
        format!(
            "`{event_name}` changed in a way the schema registry cannot \
             evolve additively — {}",
            parts.join("; ")
        ),
    )
    .with_note(
        "an additive change adds only fields that carry a default; give a \
         breaking change a new event type name instead",
    )
}

/// Read `bynk.schema.lock` from `project_root`. A missing file is a fresh
/// project (empty registry, no error) — the analogous first-ever
/// `bynk.deploy.lock` read. A present-but-unparseable or empty file is
/// corruption, not a fresh project, and fails hard (`ledger.rs`'s own
/// argument for the identical case).
pub(crate) fn read(project_root: &Path) -> Result<SchemaRegistry, String> {
    let path = project_root.join(LOCK_FILE);
    if !path.exists() {
        return Ok(SchemaRegistry {
            version: lock_version(),
            events: BTreeMap::new(),
        });
    }
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    if text.trim().is_empty() {
        return Err(format!(
            "schema registry `{}` is empty or truncated (corrupt); refusing \
             to treat it as a fresh project — restore it from version control",
            path.display()
        ));
    }
    let reg: SchemaRegistry = toml::from_str(&text).map_err(|e| {
        format!(
            "schema registry `{}` is corrupt ({e}) — restore it from version control",
            path.display()
        )
    })?;
    if reg.version != lock_version() {
        return Err(format!(
            "unsupported schema registry version {}",
            reg.version
        ));
    }
    Ok(reg)
}

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Write `bynk.schema.lock` to `project_root`, atomically (temp file +
/// `sync_all` + rename + best-effort directory fsync — `ledger.rs`'s own
/// discipline, so a crash mid-write can only leave the intact old file or the
/// intact new one). A no-op when the serialised content is byte-identical to
/// what is already on disk, so a clean rebuild never touches the file's
/// mtime or produces a spurious `git diff`.
pub(crate) fn write(project_root: &Path, reg: &SchemaRegistry) -> Result<(), String> {
    let path = project_root.join(LOCK_FILE);
    let body = toml::to_string_pretty(reg).map_err(|e| e.to_string())?;
    if let Ok(existing) = std::fs::read_to_string(&path)
        && existing == body
    {
        return Ok(());
    }

    let dir = project_root;
    let (tmp, mut file) = loop {
        let n = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let candidate = dir.join(format!(".{LOCK_FILE}.{}.{n}.tmp", std::process::id()));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(f) => break (candidate, f),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        }
    };

    let write_then_sync = file
        .write_all(body.as_bytes())
        .and_then(|()| file.sync_all());
    if let Err(e) = write_then_sync {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.to_string());
    }
    drop(file);
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.to_string());
    }
    if let Ok(dir_handle) = std::fs::File::open(dir) {
        let _ = dir_handle.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bynk_syntax::ast::{
        Annotation, AnnotationArg, BaseType, Expr, ExprId, ExprKind, Ident, RecordBody, Trivia,
    };
    use bynk_syntax::span::Span;
    use std::collections::HashMap as StdHashMap;

    fn ident(name: &str) -> Ident {
        Ident {
            name: name.to_string(),
            span: Span::default(),
        }
    }

    fn int_lit(value: i64) -> Expr {
        Expr {
            id: ExprId::SYNTHETIC,
            kind: ExprKind::IntLit {
                value,
                lexeme: value.to_string(),
            },
            span: Span::default(),
        }
    }

    fn field(name: &str, ty: TypeRef, has_default: bool) -> bynk_syntax::ast::RecordField {
        bynk_syntax::ast::RecordField {
            name: ident(name),
            type_ref: ty,
            refinement: None,
            init: has_default.then(|| int_lit(0)),
            span: Span::default(),
        }
    }

    fn schema_annotation(n: i64) -> Annotation {
        Annotation {
            name: ident("schema"),
            args: vec![AnnotationArg {
                label: None,
                value: int_lit(n),
                span: Span::default(),
            }],
            span: Span::default(),
        }
    }

    fn event(
        name: &str,
        annotations: Vec<Annotation>,
        fields: Vec<bynk_syntax::ast::RecordField>,
    ) -> EventDecl {
        EventDecl {
            name: ident(name),
            annotations,
            body: RecordBody {
                fields,
                span: Span::default(),
            },
            documentation: None,
            span: Span::default(),
            trivia: Trivia::default(),
        }
    }

    fn int_ty() -> TypeRef {
        TypeRef::Base(BaseType::Int, Span::default())
    }
    fn string_ty() -> TypeRef {
        TypeRef::Base(BaseType::String, Span::default())
    }

    fn shape(name: &str, ty: &str, default: bool) -> FieldShape {
        FieldShape {
            name: name.to_string(),
            ty: ty.to_string(),
            default,
        }
    }

    // -- canon_type -----------------------------------------------------

    #[test]
    fn canon_type_renders_base_and_generic_shapes() {
        assert_eq!(canon_type(&int_ty()), "Int");
        assert_eq!(
            canon_type(&TypeRef::Option(Box::new(string_ty()), Span::default())),
            "Option[String]"
        );
        assert_eq!(
            canon_type(&TypeRef::List(Box::new(int_ty()), Span::default())),
            "List[Int]"
        );
    }

    // -- reconcile_one ----------------------------------------------------

    #[test]
    fn no_entry_baselines_silently() {
        let current = vec![shape("orderId", "String", false)];
        match reconcile_one(&current, None) {
            Reconciled::Baseline { fields } => assert_eq!(fields, current),
            _ => panic!("expected Baseline"),
        }
    }

    #[test]
    fn unchanged_shape_keeps_stored_version() {
        let current = vec![shape("orderId", "String", false)];
        let stored = EventEntry {
            schema: 2,
            fields: current.clone(),
        };
        match reconcile_one(&current, Some(&stored)) {
            Reconciled::Unchanged { stored: v, .. } => assert_eq!(v, 2),
            _ => panic!("expected Unchanged"),
        }
    }

    #[test]
    fn additive_field_with_default_bumps_version() {
        let old = vec![shape("orderId", "String", false)];
        let new = vec![
            shape("orderId", "String", false),
            shape("region", "Region", true),
        ];
        let stored = EventEntry {
            schema: 1,
            fields: old,
        };
        match reconcile_one(&new, Some(&stored)) {
            Reconciled::Additive { bumped, .. } => assert_eq!(bumped, 2),
            _ => panic!("expected Additive"),
        }
    }

    #[test]
    fn field_removed_is_non_additive() {
        let old = vec![
            shape("orderId", "String", false),
            shape("region", "Region", false),
        ];
        let new = vec![shape("orderId", "String", false)];
        let stored = EventEntry {
            schema: 1,
            fields: old,
        };
        match reconcile_one(&new, Some(&stored)) {
            Reconciled::NonAdditive { removed, .. } => {
                assert_eq!(removed, vec!["region".to_string()])
            }
            _ => panic!("expected NonAdditive"),
        }
    }

    #[test]
    fn field_retyped_is_non_additive() {
        let old = vec![shape("orderId", "String", false)];
        let new = vec![shape("orderId", "Int", false)];
        let stored = EventEntry {
            schema: 1,
            fields: old,
        };
        match reconcile_one(&new, Some(&stored)) {
            Reconciled::NonAdditive { retyped, .. } => {
                assert_eq!(retyped, vec!["orderId".to_string()])
            }
            _ => panic!("expected NonAdditive"),
        }
    }

    #[test]
    fn field_added_without_default_is_non_additive() {
        let old = vec![shape("orderId", "String", false)];
        let new = vec![
            shape("orderId", "String", false),
            shape("region", "Region", false),
        ];
        let stored = EventEntry {
            schema: 1,
            fields: old,
        };
        match reconcile_one(&new, Some(&stored)) {
            Reconciled::NonAdditive {
                added_without_default,
                ..
            } => assert_eq!(added_without_default, vec!["region".to_string()]),
            _ => panic!("expected NonAdditive"),
        }
    }

    #[test]
    fn a_field_losing_its_default_is_non_additive() {
        // An older wire event that omitted `region` (relying on the default)
        // could no longer decode once the default is gone — the same
        // forward-compatibility break as adding a field without one, from
        // the other direction.
        let old = vec![
            shape("orderId", "String", false),
            shape("region", "String", true),
        ];
        let new = vec![
            shape("orderId", "String", false),
            shape("region", "String", false),
        ];
        let stored = EventEntry {
            schema: 1,
            fields: old,
        };
        match reconcile_one(&new, Some(&stored)) {
            Reconciled::NonAdditive { lost_default, .. } => {
                assert_eq!(lost_default, vec!["region".to_string()])
            }
            _ => panic!("expected NonAdditive"),
        }
    }

    // -- reconcile (full pipeline, with real EventDecls) -------------------

    fn table_with(events: Vec<(&str, EventDecl)>) -> UnitTable {
        UnitTable {
            kind: None,
            types: StdHashMap::new(),
            fns: StdHashMap::new(),
            methods: StdHashMap::new(),
            capabilities: StdHashMap::new(),
            providers: StdHashMap::new(),
            services: StdHashMap::new(),
            agents: StdHashMap::new(),
            actors: StdHashMap::new(),
            exported_capabilities: Default::default(),
            events: events
                .into_iter()
                .map(|(n, e)| (n.to_string(), e))
                .collect(),
        }
    }

    #[test]
    fn reconcile_baselines_a_brand_new_event_at_its_declared_annotation() {
        let e = event(
            "PaymentConfirmed",
            vec![schema_annotation(3)],
            vec![field("orderId", string_ty(), false)],
        );
        let mut units = StdHashMap::new();
        units.insert(
            "commerce.order".to_string(),
            table_with(vec![("PaymentConfirmed", e)]),
        );
        let existing = SchemaRegistry {
            version: lock_version(),
            events: BTreeMap::new(),
        };
        let mut errors = Vec::new();
        let (updated, effective) = reconcile(&existing, &units, &mut errors);
        assert!(errors.is_empty(), "a first-ever compile must not error");
        assert_eq!(effective.get("commerce.order.PaymentConfirmed"), Some(&3));
        assert_eq!(updated.events["commerce.order.PaymentConfirmed"].schema, 3);
    }

    #[test]
    fn reconcile_rejects_a_mismatched_schema_annotation_on_an_unchanged_event() {
        let e = event(
            "PaymentConfirmed",
            vec![schema_annotation(5)],
            vec![field("orderId", string_ty(), false)],
        );
        let mut units = StdHashMap::new();
        units.insert(
            "commerce.order".to_string(),
            table_with(vec![("PaymentConfirmed", e)]),
        );
        let mut events = BTreeMap::new();
        events.insert(
            "commerce.order.PaymentConfirmed".to_string(),
            EventEntry {
                schema: 2,
                fields: vec![shape("orderId", "String", false)],
            },
        );
        let existing = SchemaRegistry {
            version: lock_version(),
            events,
        };
        let mut errors = Vec::new();
        let (_, effective) = reconcile(&existing, &units, &mut errors);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].category, "bynk.event.schema_version_mismatch");
        // The registry's computed version wins as the effective one, not the
        // author's stale annotation.
        assert_eq!(effective.get("commerce.order.PaymentConfirmed"), Some(&2));
    }

    #[test]
    fn reconcile_auto_bumps_an_unannotated_additive_change() {
        let e = event(
            "OrderCancelled",
            vec![],
            vec![
                field("orderId", string_ty(), false),
                field("reason", string_ty(), true),
            ],
        );
        let mut units = StdHashMap::new();
        units.insert(
            "commerce.order".to_string(),
            table_with(vec![("OrderCancelled", e)]),
        );
        let mut events = BTreeMap::new();
        events.insert(
            "commerce.order.OrderCancelled".to_string(),
            EventEntry {
                schema: 1,
                fields: vec![shape("orderId", "String", false)],
            },
        );
        let existing = SchemaRegistry {
            version: lock_version(),
            events,
        };
        let mut errors = Vec::new();
        let (updated, effective) = reconcile(&existing, &units, &mut errors);
        assert!(errors.is_empty());
        assert_eq!(effective.get("commerce.order.OrderCancelled"), Some(&2));
        assert_eq!(updated.events["commerce.order.OrderCancelled"].schema, 2);
    }

    #[test]
    fn reconcile_rejects_a_non_additive_change_and_keeps_the_old_entry() {
        let e = event(
            "OrderCancelled",
            vec![],
            vec![field("orderId", string_ty(), false)],
        );
        let mut units = StdHashMap::new();
        units.insert(
            "commerce.order".to_string(),
            table_with(vec![("OrderCancelled", e)]),
        );
        let mut events = BTreeMap::new();
        events.insert(
            "commerce.order.OrderCancelled".to_string(),
            EventEntry {
                schema: 4,
                fields: vec![
                    shape("orderId", "String", false),
                    shape("reason", "String", false),
                ],
            },
        );
        let existing = SchemaRegistry {
            version: lock_version(),
            events,
        };
        let mut errors = Vec::new();
        let (updated, effective) = reconcile(&existing, &units, &mut errors);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].category, "bynk.event.non_additive_schema_change");
        assert_eq!(effective.get("commerce.order.OrderCancelled"), Some(&4));
        assert_eq!(updated.events["commerce.order.OrderCancelled"].schema, 4);
    }

    #[test]
    fn a_stale_key_for_a_renamed_event_is_dropped_silently() {
        // The old table had `PaymentConfirmed`; the new one only has
        // `PaymentConfirmedV2` (the track's prescribed rename-for-breaking-
        // change path). No diagnostic, and the old key does not survive into
        // the updated document.
        let e = event(
            "PaymentConfirmedV2",
            vec![],
            vec![field("orderId", string_ty(), false)],
        );
        let mut units = StdHashMap::new();
        units.insert(
            "commerce.order".to_string(),
            table_with(vec![("PaymentConfirmedV2", e)]),
        );
        let mut events = BTreeMap::new();
        events.insert(
            "commerce.order.PaymentConfirmed".to_string(),
            EventEntry {
                schema: 3,
                fields: vec![shape("orderId", "String", false)],
            },
        );
        let existing = SchemaRegistry {
            version: lock_version(),
            events,
        };
        let mut errors = Vec::new();
        let (updated, _) = reconcile(&existing, &units, &mut errors);
        assert!(errors.is_empty());
        assert!(
            !updated
                .events
                .contains_key("commerce.order.PaymentConfirmed")
        );
        assert!(
            updated
                .events
                .contains_key("commerce.order.PaymentConfirmedV2")
        );
    }

    // -- read/write round-trip -------------------------------------------

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "bynk-schema-registry-test-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn read_of_an_absent_file_is_an_empty_registry() {
        let dir = temp_dir("absent");
        let reg = read(&dir).unwrap();
        assert!(reg.events.is_empty());
    }

    #[test]
    fn write_then_read_round_trips() {
        let dir = temp_dir("roundtrip");
        let mut events = BTreeMap::new();
        events.insert(
            "commerce.order.PaymentConfirmed".to_string(),
            EventEntry {
                schema: 2,
                fields: vec![shape("orderId", "String", false)],
            },
        );
        let reg = SchemaRegistry {
            version: lock_version(),
            events,
        };
        write(&dir, &reg).unwrap();
        let read_back = read(&dir).unwrap();
        assert_eq!(read_back, reg);
    }

    #[test]
    fn write_is_a_no_op_when_content_is_unchanged() {
        let dir = temp_dir("noop-write");
        let reg = SchemaRegistry {
            version: lock_version(),
            events: BTreeMap::new(),
        };
        write(&dir, &reg).unwrap();
        let mtime_before = std::fs::metadata(lock_path(&dir))
            .unwrap()
            .modified()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        write(&dir, &reg).unwrap();
        let mtime_after = std::fs::metadata(lock_path(&dir))
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(
            mtime_before, mtime_after,
            "unchanged content must not rewrite the file"
        );
    }

    #[test]
    fn a_truncated_file_is_corruption_not_a_fresh_project() {
        let dir = temp_dir("truncated");
        std::fs::write(lock_path(&dir), "   \n").unwrap();
        let err = read(&dir).unwrap_err();
        assert!(err.contains("restore it from version control"));
    }

    #[test]
    fn an_unparseable_file_is_corruption() {
        let dir = temp_dir("unparseable");
        std::fs::write(lock_path(&dir), "not valid toml {{{").unwrap();
        let err = read(&dir).unwrap_err();
        assert!(err.contains("corrupt"));
    }
}
