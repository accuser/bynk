use super::*;
use bynk_check::context_checks::{
    reject_fn_types, type_ref_is_held, validate_store_field_value_types,
};
use std::sync::Arc;

/// v0.19: the lock violation a deployment unit's native-platform set implies
/// under the selected `--platform`, if any. Pure — unit-tested below with
/// synthetic sets (the conflict arm is not yet reachable end-to-end while
/// only one platform ships native capabilities).
fn lock_violation(
    native: &std::collections::BTreeMap<Platform, String>,
    selected: Platform,
) -> Option<LockViolation> {
    let mut platforms = native.iter();
    let (first, first_unit) = platforms.next()?;
    if let Some((second, second_unit)) = platforms.next() {
        return Some(LockViolation::Conflict {
            a: (*first, first_unit.clone()),
            b: (*second, second_unit.clone()),
        });
    }
    if *first != selected {
        return Some(LockViolation::Required {
            needed: *first,
            unit: first_unit.clone(),
        });
    }
    None
}

/// A platform-lock violation (v0.19, `bynk.target.*`).
#[derive(Debug, PartialEq, Eq)]
enum LockViolation {
    /// The deployment unit needs `needed` but another platform is selected.
    Required { needed: Platform, unit: String },
    /// The deployment unit's closure spans two mutually-exclusive platforms.
    Conflict {
        a: (Platform, String),
        b: (Platform, String),
    },
}

/// v0.19 (decisions 0017/0024): enforce the platform lock per deployment
/// unit — each context under `--target workers`, the whole program under
/// `bundle` (co-location shares the lock).
#[allow(clippy::too_many_arguments)]
pub(crate) fn check_platform_lock(
    target: BuildTarget,
    selected: Platform,
    parsed: &[ParsedFile],
    groups: &BTreeMap<String, Vec<usize>>,
    kinds: &BTreeMap<String, UnitKind>,
    unit_tables: &HashMap<String, UnitTable>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_consumes_aliases: &HashMap<String, HashMap<String, String>>,
    unit_flattened: &HashMap<String, HashMap<String, String>>,
    errors: &mut Vec<CompileError>,
) {
    // In-browser track, slice 2: `browser` is a Bundle-only platform — a browser
    // cannot do the Workers wire-call model (Service Bindings, Durable Objects,
    // cross-context wire calls). Reject the combination up front, before the
    // per-unit native-platform lock below, which is moot for an invalid build.
    if selected == Platform::Browser && target == BuildTarget::Workers {
        errors.push(
            CompileError::new(
                "bynk.target.browser_bundle_only",
                Span::default(),
                "`--platform browser` builds only the in-process `Bundle` topology, but `--target workers` was selected; a browser cannot run the Workers wire-call model",
            )
            .with_note("build the browser target with `--target bundle` (the default)"),
        );
        return;
    }
    // v0.104 (real-time track slice 3b): the `from websocket` Workers mapping (the
    // Durable Object hibernatable upgrade) is now emitted, so the 3a platform-lock
    // that gated it off is removed.
    // Per-context native sets, with the context name kept for spans/messages.
    let mut per_context: Vec<(String, std::collections::BTreeMap<Platform, String>)> = Vec::new();
    let mut names: Vec<&String> = groups.keys().collect();
    names.sort();
    for name in names {
        if kinds.get(name.as_str()) != Some(&UnitKind::Context) {
            continue;
        }
        let Some(table) = unit_tables.get(name.as_str()) else {
            continue;
        };
        let native = native_platforms_of_context(
            name,
            table,
            unit_tables,
            unit_consumes,
            unit_consumes_aliases,
            unit_flattened,
        );
        if !native.is_empty() {
            per_context.push((name.clone(), native));
        }
    }
    // The deployment units to check: per-context under workers; their union
    // under bundle (the whole program co-locates).
    let units: Vec<(String, std::collections::BTreeMap<Platform, String>)> = match target {
        BuildTarget::Workers => per_context,
        BuildTarget::Bundle => {
            let mut union = std::collections::BTreeMap::new();
            let mut owner: Option<String> = None;
            for (ctx, native) in per_context {
                owner.get_or_insert(ctx);
                for (p, unit) in native {
                    union.entry(p).or_insert(unit);
                }
            }
            match owner {
                Some(ctx) if !union.is_empty() => vec![(ctx, union)],
                _ => Vec::new(),
            }
        }
    };
    for (ctx, native) in units {
        let Some(violation) = lock_violation(&native, selected) else {
            continue;
        };
        let span_for = |unit: &str| {
            groups
                .get(&ctx)
                .and_then(|idx| consumes_span_of(parsed, idx, unit))
                .map(|(_, s)| s)
                .unwrap_or_default()
        };
        match violation {
            LockViolation::Required { needed, unit } => {
                errors.push(
                    CompileError::new(
                        "bynk.target.vendor_required",
                        span_for(&unit),
                        format!(
                            "context `{ctx}` uses the platform-native capabilities of `{unit}`, which run only on the `{}` platform, but the build selects `--platform {}`",
                            needed.as_str(),
                            selected.as_str(),
                        ),
                    )
                    .with_note(
                        "build with the matching `--platform`, or remove the platform-native dependency to stay portable",
                    ),
                );
            }
            LockViolation::Conflict { a, b } => {
                errors.push(
                    CompileError::new(
                        "bynk.target.vendor_conflict",
                        span_for(&a.1),
                        format!(
                            "one deployment unit (via context `{ctx}`) uses platform-native capabilities from two mutually-exclusive platforms: `{}` (from `{}`) and `{}` (from `{}`)",
                            a.0.as_str(),
                            a.1,
                            b.0.as_str(),
                            b.1,
                        ),
                    )
                    .with_note(
                        "split the consumers into separate deployment units (`--target workers`), or remove one of the platform-native dependencies",
                    ),
                );
            }
        }
    }
}

/// v0.20a: apply the function-type boundary confinement to every serialisable
/// or boundary-crossing position in a file's items: record fields and sum
/// payloads (types can cross contexts and persist), service/agent handler
/// signatures (the Workers wire), capability operation signatures (kept out
/// in v0.20a — see ADR 0030), agent state fields, and agent keys. Free `fn`
/// signatures are deliberately NOT walked — they are the non-boundary home
/// of function types.
///
/// #696: each diagnostic is paired with the project-relative `identity_path` of
/// the file whose items produced it, so the CLI renders it against that file's
/// source.
pub(crate) fn check_function_type_boundaries(
    parsed: &[ParsedFile],
) -> Vec<(PathBuf, CompileError)> {
    // v0.174 (#592): the boundary check now also rejects a *recursive* generic
    // record (`reject_fn_types`' `App` arm), which needs the type declarations to
    // walk the containment graph. Build the project-wide table once — a generic
    // referenced from one file may be declared in another.
    let types = collect_type_decls(parsed.iter().flat_map(|pf| pf.items()));
    let mut attributed: Vec<(PathBuf, CompileError)> = Vec::new();
    for pf in parsed {
        let mut file_errors: Vec<CompileError> = Vec::new();
        check_function_type_boundary_items(pf.items(), &types, &mut file_errors);
        attributed.extend(file_errors.into_iter().map(|e| (pf.identity_path(), e)));
    }
    attributed
}

/// v0.174 (#592): a `name -> TypeDecl` table over a set of items, for the
/// recursive-generic boundary walk.
pub(crate) fn collect_type_decls<'a>(
    items: impl Iterator<Item = &'a CommonsItem>,
) -> std::collections::HashMap<String, Arc<TypeDecl>> {
    let mut out = std::collections::HashMap::new();
    for item in items {
        match item {
            CommonsItem::Type(t) => {
                out.entry(t.name.name.clone())
                    .or_insert_with(|| Arc::new(t.clone()));
            }
            // Events track, slice 0 (spine #936): an event's synthetic
            // `TypeDecl` joins the same table, so a field referencing an
            // event type recurses into it exactly like any other type.
            CommonsItem::Event(e) => {
                out.entry(e.name.name.clone())
                    .or_insert_with(|| Arc::new(e.as_type_decl()));
            }
            _ => {}
        }
    }
    out
}

/// Item-level body of the boundary confinement, shared with the single-file
/// (legacy) compile path in `bynkc`'s `lib.rs`.
pub fn check_function_type_boundary_items(
    items: &[CommonsItem],
    types: &std::collections::HashMap<String, Arc<TypeDecl>>,
    errors: &mut Vec<CompileError>,
) {
    {
        for item in items {
            match item {
                CommonsItem::Type(t) => match &t.body {
                    TypeBody::Record(r) => {
                        for f in &r.fields {
                            reject_fn_types(&f.type_ref, "a record field", types, errors);
                        }
                    }
                    TypeBody::Sum(s) => {
                        for v in &s.variants {
                            for p in &v.payload {
                                reject_fn_types(
                                    &p.type_ref,
                                    "a sum-variant payload",
                                    types,
                                    errors,
                                );
                            }
                        }
                    }
                    TypeBody::Refined { .. } | TypeBody::Opaque { .. } => {}
                },
                // Events track, slice 0 (spine #936): an event's fields are
                // boundary values (an emission crosses a context boundary),
                // so the same record-field rule applies as for a `type`.
                CommonsItem::Event(e) => {
                    for f in &e.body.fields {
                        reject_fn_types(&f.type_ref, "an event field", types, errors);
                    }
                }
                CommonsItem::Capability(c) => {
                    for op in &c.ops {
                        for p in &op.params {
                            reject_fn_types(
                                &p.type_ref,
                                "a capability operation signature",
                                types,
                                errors,
                            );
                        }
                        // v0.102 (§2.9.1): a capability operation may *produce* a
                        // held value — it is the canonical held source — so an
                        // `Effect[Connection[F]]` return is admitted.
                        if !type_ref_is_held(&op.return_type) {
                            reject_fn_types(
                                &op.return_type,
                                "a capability operation signature",
                                types,
                                errors,
                            );
                        }
                    }
                }
                CommonsItem::Service(s) => {
                    for h in &s.handlers {
                        for p in &h.params {
                            // v0.102 (§2.9.4): the framework may supply a held
                            // value as a handler parameter (the `on open`
                            // connection), so a `Connection[F]` parameter is
                            // admitted.
                            if !type_ref_is_held(&p.type_ref) {
                                reject_fn_types(
                                    &p.type_ref,
                                    "a service handler signature",
                                    types,
                                    errors,
                                );
                            }
                        }
                        reject_fn_types(
                            &h.return_type,
                            "a service handler signature",
                            types,
                            errors,
                        );
                    }
                }
                CommonsItem::Agent(a) => {
                    reject_fn_types(&a.key_type, "an agent key", types, errors);
                    for f in &a.store_fields {
                        validate_store_field_value_types(f, types, errors);
                    }
                    for h in &a.handlers {
                        for p in &h.params {
                            // v0.102 (§2.9.4): a held value may be transferred to
                            // an agent handler as a parameter.
                            if !type_ref_is_held(&p.type_ref) {
                                reject_fn_types(
                                    &p.type_ref,
                                    "an agent handler signature",
                                    types,
                                    errors,
                                );
                            }
                        }
                        reject_fn_types(
                            &h.return_type,
                            "an agent handler signature",
                            types,
                            errors,
                        );
                    }
                }
                CommonsItem::Actor(a) => {
                    if let Some(id) = &a.identity {
                        reject_fn_types(id, "an actor identity type", types, errors);
                    }
                }
                // slice 1: `MessageEntry.code`/`.template` are plain string
                // literals, no fn-type-bearing fields to reject here.
                CommonsItem::Fn(_) | CommonsItem::Provider(_) | CommonsItem::Messages(_) => {}
            }
        }
    }
}
#[cfg(test)]
mod platform_lock_tests {
    use super::{LockViolation, Platform, lock_violation};
    use std::collections::BTreeMap;

    fn native(entries: &[(Platform, &str)]) -> BTreeMap<Platform, String> {
        entries
            .iter()
            .map(|(p, u)| (*p, (*u).to_string()))
            .collect()
    }

    #[test]
    fn empty_closure_imposes_no_lock() {
        assert_eq!(lock_violation(&native(&[]), Platform::Node), None);
    }

    #[test]
    fn matching_platform_is_fine() {
        let n = native(&[(Platform::Cloudflare, "bynk.cloudflare")]);
        assert_eq!(lock_violation(&n, Platform::Cloudflare), None);
    }

    #[test]
    fn mismatched_platform_is_required() {
        let n = native(&[(Platform::Cloudflare, "bynk.cloudflare")]);
        assert_eq!(
            lock_violation(&n, Platform::Node),
            Some(LockViolation::Required {
                needed: Platform::Cloudflare,
                unit: "bynk.cloudflare".to_string(),
            })
        );
    }

    // The conflict arm is not yet reachable end-to-end (only one platform
    // ships native capabilities until `bynk.aws`); the rule is exercised here
    // with a synthetic two-platform set so it does not ship untested
    // (proposal v0.19, review call).
    #[test]
    fn two_platforms_conflict_regardless_of_selection() {
        let n = native(&[
            (Platform::Cloudflare, "bynk.cloudflare"),
            (Platform::Node, "bynk.synthetic"),
        ]);
        let v = lock_violation(&n, Platform::Cloudflare);
        assert_eq!(
            v,
            Some(LockViolation::Conflict {
                a: (Platform::Cloudflare, "bynk.cloudflare".to_string()),
                b: (Platform::Node, "bynk.synthetic".to_string()),
            })
        );
    }
}
