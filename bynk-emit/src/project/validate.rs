use super::*;
use crate::emitter;
use bynk_check::context_checks::{
    reject_fn_types, type_ref_is_held, type_ref_to_display, validate_store_field_value_types,
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

/// message-bundles slice 1 (#859): per-commons validation for `messages`
/// blocks — commons-only legality, exactly one `@reference` annotation
/// across every `messages` block in the commons, a within-block duplicate
/// `code`, and (since nothing in this compiler auto-injects a `uses`
/// clause) that a commons declaring `messages` also `uses bynk.locale`
/// (the generated bundle-scoped `render`'s fallback needs it in scope) and
/// `uses bynk.locale.types` (locale-negotiation-slice-2 follow-up, #886 —
/// the generated `render`'s own signature names `LocaleTag`/`Message`).
///
/// message-bundles slice 2 (#874): once cardinality confirms exactly one
/// `@reference` block, a second pass diffs every other declared locale
/// against it — reference-bundle completeness (`bynk.messages.incomplete`,
/// one diagnostic per missing `(locale, code)` witness, mirroring
/// `bynk.types.non_exhaustive_match`'s own one-witness-per-diagnostic
/// convention) and cross-locale placeholder-*set* agreement
/// (`bynk.messages.placeholder_mismatch`, only for codes present in both —
/// a missing code is `incomplete`'s job, not this one's). Two blocks
/// declaring the same locale tag are rejected outright
/// (`bynk.resolve.duplicate_message_locale`, PR #875 review) — the emitter
/// has no dedup of its own, so a silent last-wins here would let a hard
/// `tsc` redeclare error (two colliding `const __messages_<tag>`
/// declarations) through instead.
pub(crate) fn check_messages_bundles(
    parsed: &[ParsedFile],
    groups: &BTreeMap<String, Vec<usize>>,
    kinds: &BTreeMap<String, UnitKind>,
    unit_uses: &HashMap<String, Vec<String>>,
    errors: &mut ErrorSink,
) {
    for (name, indices) in groups {
        let mut first_messages: Option<(usize, Span)> = None;
        let mut reference_sites: Vec<(usize, Span)> = Vec::new();
        let mut reference_block: Option<(usize, &MessagesDecl)> = None;
        let mut by_tag: HashMap<&str, (usize, &MessagesDecl)> = HashMap::new();
        for &i in indices {
            for item in parsed[i].items() {
                let CommonsItem::Messages(m) = item else {
                    continue;
                };
                if first_messages.is_none() {
                    first_messages = Some((i, m.span));
                }
                if kinds.get(name) != Some(&UnitKind::Commons) {
                    errors.push_for(
                        Some(&parsed[i].identity_path()),
                        CompileError::new(
                            "bynk.messages.outside_commons",
                            m.span,
                            "`messages` declarations are only allowed inside a commons, not a context or adapter",
                        ),
                    );
                    continue;
                }
                // #899: the tag is a `LocaleTag` string literal, checked here
                // against `LocaleTag`'s own refinement (read from the
                // firstparty `bynk.locale.types` source, so the pattern has one
                // definition). An invalid tag would otherwise reach `Intl` at
                // runtime as `new Intl.PluralRules("xx")`, which throws — the
                // opposite of `render`'s totality contract.
                if !bynk_check::checker::locale_tag_accepts(&m.tag) {
                    let pattern = bynk_check::checker::locale_tag_pattern().unwrap_or("");
                    errors.push_for(
                        Some(&parsed[i].identity_path()),
                        CompileError::new(
                            "bynk.messages.invalid_locale_tag",
                            m.tag_span,
                            format!(
                                "\"{}\" is not a valid `LocaleTag` — it must match the pattern `{}`",
                                m.tag, pattern
                            ),
                        ),
                    );
                }
                // message-bundles slice 2 (#874, PR #875 review): two blocks
                // declaring the same locale tag are rejected, not
                // last-write-wins — the emitter (`emit_messages_bundle`) has no
                // dedup of its own and would emit two colliding table entries
                // under one object key, a hard `tsc` error. Mirrors
                // `bynk.resolve.duplicate_fn`'s own shape: only the *first*
                // occurrence seeds `by_tag`, so a third duplicate still reports
                // against the original, not the second.
                if let Some(&(_, prev)) = by_tag.get(m.tag.as_str()) {
                    errors.push_for(
                        Some(&parsed[i].identity_path()),
                        CompileError::new(
                            "bynk.resolve.duplicate_message_locale",
                            m.tag_span,
                            format!("locale \"{}\" is already declared in this bundle", m.tag),
                        )
                        .with_label(prev.tag_span, "previously declared here"),
                    );
                } else {
                    by_tag.insert(m.tag.as_str(), (i, m));
                }
                for ann in &m.annotations {
                    if ann.name.name == "reference" {
                        reference_sites.push((i, ann.span));
                        reference_block = Some((i, m));
                    }
                }
                let mut seen: HashMap<&str, Span> = HashMap::new();
                for entry in &m.entries {
                    if let Some(prev) = seen.get(entry.code.as_str()) {
                        errors.push_for(
                            Some(&parsed[i].identity_path()),
                            CompileError::new(
                                "bynk.resolve.duplicate_message_code",
                                entry.code_span,
                                format!(
                                    "message code \"{}\" is already declared in this block",
                                    entry.code
                                ),
                            )
                            .with_label(*prev, "previously declared here"),
                        );
                    } else {
                        seen.insert(entry.code.as_str(), entry.code_span);
                    }
                    // message-bundles slice 3 (#878): runs unconditionally,
                    // once per entry, regardless of `@reference` cardinality
                    // — malformed ICU syntax shouldn't wait on cardinality
                    // being resolved first.
                    check_entry_icu_syntax(entry, Some(&parsed[i].identity_path()), errors);
                }
            }
        }
        let Some((first_i, first_span)) = first_messages else {
            continue;
        };
        if kinds.get(name) != Some(&UnitKind::Commons) {
            // Already reported above (outside_commons) for every block;
            // cardinality/uses checks don't apply to a non-commons unit.
            continue;
        }
        match reference_sites.len() {
            0 => {
                errors.push_for(
                    Some(&parsed[first_i].identity_path()),
                    CompileError::new(
                        "bynk.messages.missing_reference",
                        first_span,
                        "a message bundle must have exactly one `@reference` block; none found",
                    ),
                );
            }
            1 => {
                // message-bundles slice 2 (#874): "the reference" is only
                // well-defined here — 0 or 2+ already reported their own
                // diagnostic above, and completeness/placeholder-agreement
                // against an ambiguous or absent reference would be noise.
                let (_, reference) = reference_block
                    .expect("reference_sites.len() == 1 implies reference_block is Some");
                // Sorted for deterministic diagnostic order — `by_tag`'s
                // HashMap iteration is not otherwise stable across runs.
                let mut sorted_tags: Vec<&&str> = by_tag.keys().collect();
                sorted_tags.sort();
                for &&tag in &sorted_tags {
                    let &(locale_i, locale_m) = &by_tag[tag];
                    if tag == reference.tag.as_str() {
                        continue;
                    }
                    for ref_entry in &reference.entries {
                        let Some(locale_entry) =
                            locale_m.entries.iter().find(|e| e.code == ref_entry.code)
                        else {
                            errors.push_for(
                                Some(&parsed[locale_i].identity_path()),
                                CompileError::new(
                                    "bynk.messages.incomplete",
                                    locale_m.span,
                                    format!(
                                        "locale \"{tag}\" is missing code \"{}\", declared by the reference locale \"{}\"",
                                        ref_entry.code, reference.tag
                                    ),
                                ),
                            );
                            continue;
                        };
                        let ref_names = emitter::placeholder_names(&ref_entry.template);
                        let locale_names = emitter::placeholder_names(&locale_entry.template);
                        if ref_names != locale_names {
                            errors.push_for(
                                Some(&parsed[locale_i].identity_path()),
                                CompileError::new(
                                    "bynk.messages.placeholder_mismatch",
                                    locale_entry.template_span,
                                    format!(
                                        "locale \"{tag}\"'s template for code \"{}\" uses placeholders {locale_names:?}, but the reference locale \"{}\"'s uses {ref_names:?}",
                                        ref_entry.code, reference.tag
                                    ),
                                ),
                            );
                        }
                        // message-bundles slice 3 (#878, Decision D): a name
                        // present in both templates must also agree on ICU
                        // format *kind* (plain/plural/select/number/date) —
                        // a UI can't sanely alternate that per locale. A
                        // missing name is `placeholder_mismatch`'s job, not
                        // this one's; a malformed template's kinds are
                        // silently absent from `template_format_kinds`
                        // (already reported once by `check_entry_icu_syntax`
                        // above, never double-reported here).
                        let ref_kinds = emitter::template_format_kinds(&ref_entry.template);
                        let locale_kinds = emitter::template_format_kinds(&locale_entry.template);
                        for (pname, ref_kind) in &ref_kinds {
                            let Some(locale_kind) = locale_kinds.get(pname) else {
                                continue;
                            };
                            if locale_kind != ref_kind {
                                errors.push_for(
                                    Some(&parsed[locale_i].identity_path()),
                                    CompileError::new(
                                        "bynk.messages.format_mismatch",
                                        locale_entry.template_span,
                                        format!(
                                            "locale \"{tag}\"'s placeholder \"{pname}\" in code \"{}\" is formatted as {}, but the reference locale \"{}\"'s is {}",
                                            ref_entry.code,
                                            locale_kind.as_str(),
                                            reference.tag,
                                            ref_kind.as_str(),
                                        ),
                                    ),
                                );
                            }
                        }
                    }
                }
            }
            _ => {
                let (_, first_ref_span) = reference_sites[0];
                for &(i, span) in &reference_sites[1..] {
                    errors.push_for(
                        Some(&parsed[i].identity_path()),
                        CompileError::new(
                            "bynk.messages.multiple_reference",
                            span,
                            "a message bundle must have exactly one `@reference` block; found more than one",
                        )
                        .with_label(first_ref_span, "first `@reference` here"),
                    );
                }
            }
        }
        // Locale-negotiation-slice-2 follow-up (#886): the synthetic `render`
        // this commons gets (`synthetic_render_fn`, symbols.rs) names
        // `LocaleTag`/`Message` by `TypeRef::Named` — real, resolved
        // references, not bypassed — so both `bynk.locale` (for `render`
        // itself) and `bynk.locale.types` (for the types its signature
        // names) must be `uses`d. Kept as one diagnostic, not two: a message
        // bundle always needs both together, so splitting the code would
        // just be two author-facing fixes for one underlying requirement.
        let targets = unit_uses.get(name);
        let has_locale_uses = targets.is_some_and(|targets| {
            targets
                .iter()
                .any(|t| t == bynk_check::firstparty::LOCALE_UNIT)
        });
        let has_locale_types_uses = targets.is_some_and(|targets| {
            targets
                .iter()
                .any(|t| t == bynk_check::firstparty::LOCALE_TYPES_UNIT)
        });
        if !has_locale_uses || !has_locale_types_uses {
            let missing = match (has_locale_uses, has_locale_types_uses) {
                (false, false) => "`bynk.locale` and `bynk.locale.types`",
                (false, true) => "`bynk.locale`",
                (true, false) => "`bynk.locale.types`",
                (true, true) => unreachable!("at least one of the two is missing here"),
            };
            errors.push_for(
                Some(&parsed[first_i].identity_path()),
                CompileError::new(
                    "bynk.messages.missing_locale_dependency",
                    first_span,
                    format!("a commons declaring `messages` must also `uses` {missing}"),
                ),
            );
        }
    }
}

/// Locale capability track, slice 2 (#882): a context whose direct `uses`
/// reaches two or more message-bundle commons has no principled single
/// answer for what `Locale.current()` should negotiate against — but this
/// is only worth diagnosing when the context actually `consumes bynk {
/// Locale }` at all; a context with 2+ bundles that never touches `Locale`
/// has nothing ambiguous to resolve.
pub(crate) fn check_locale_bundle_ambiguity(
    parsed: &[ParsedFile],
    groups: &BTreeMap<String, Vec<usize>>,
    kinds: &BTreeMap<String, UnitKind>,
    unit_uses: &HashMap<String, Vec<String>>,
    unit_flattened: &HashMap<String, HashMap<String, String>>,
    errors: &mut ErrorSink,
) {
    for (name, indices) in groups {
        if kinds.get(name) != Some(&UnitKind::Context) {
            continue;
        }
        let bynk_check::symbols::ContextMessageBundle::Many(bundles) =
            bynk_check::symbols::detect_context_message_bundle(
                name, unit_uses, groups, kinds, parsed,
            )
        else {
            continue;
        };
        let consumes_locale = unit_flattened
            .get(name)
            .and_then(|m| m.get("Locale"))
            .is_some_and(|owner| owner == bynk_check::firstparty::BYNK_UNIT);
        if !consumes_locale {
            continue;
        }
        for &i in indices {
            for c in parsed[i].consumes() {
                if c.target.joined() != bynk_check::firstparty::BYNK_UNIT {
                    continue;
                }
                let Some(locale_ident) = c.selected.iter().flatten().find(|id| id.name == "Locale")
                else {
                    continue;
                };
                let mut err = CompileError::new(
                    "bynk.locale.multiple_message_bundles",
                    locale_ident.span,
                    format!(
                        "context `{name}` uses {} message bundles ({}) — `Locale.current()` has no single bundle to negotiate against",
                        bundles.len(),
                        bundles.join(", "),
                    ),
                );
                for &j in indices {
                    for u in parsed[j].uses() {
                        if bundles.contains(&u.target.joined()) {
                            err = err
                                .with_label(u.span, format!("`{}` used here", u.target.joined()));
                        }
                    }
                }
                errors.push_for(Some(&parsed[i].identity_path()), err);
            }
        }
    }
}

/// Events track, slice 0 (spine #936): a `from Events(E)` subscription must
/// name a real, declared event — owned either by this context or by a
/// context it `consumes` (mirroring `discover_event_subscribers`'s own
/// ownership resolution, `project.rs`, which silently drops an unresolvable
/// subscription rather than diagnosing it). Runs at the project-wide phase
/// (needs `unit_tables` + `unit_consumes` together, unlike the local, per-
/// context `check_service_protocols`), alongside the other cross-unit checks
/// that need the same two maps.
pub(crate) fn check_event_subscriptions(
    parsed: &[ParsedFile],
    groups: &BTreeMap<String, Vec<usize>>,
    kinds: &BTreeMap<String, UnitKind>,
    unit_tables: &HashMap<String, UnitTable>,
    unit_consumes: &HashMap<String, Vec<String>>,
    unit_uses: &HashMap<String, Vec<String>>,
    errors: &mut ErrorSink,
) {
    for (name, indices) in groups {
        if kinds.get(name) != Some(&UnitKind::Context) {
            continue;
        }
        let consumed = unit_consumes.get(name).cloned().unwrap_or_default();
        for &i in indices {
            for item in parsed[i].items() {
                let CommonsItem::Service(s) = item else {
                    continue;
                };
                let ServiceProtocol::Events {
                    event_type,
                    pattern,
                    schema_dispatch,
                } = &s.protocol
                else {
                    continue;
                };
                // Events track, slice 4 (spine #936): `via schema(N)`'s
                // legality needs nothing about the subscribed event itself
                // (unlike the payload pattern below), so it's checked
                // independently of whether the subscription even resolves.
                if let Some(dispatch) = schema_dispatch {
                    check_schema_dispatch(dispatch, &parsed[i].identity_path(), errors);
                }
                let TypeRef::Named(id) = event_type else {
                    continue;
                };
                let owner_locally = unit_tables
                    .get(name)
                    .filter(|t| t.events.contains_key(&id.name))
                    .map(|_| name.clone());
                let owner_consumed = consumed.iter().find(|c| {
                    unit_tables
                        .get(*c)
                        .is_some_and(|t| t.events.contains_key(&id.name))
                });
                let owner = owner_locally
                    .as_deref()
                    .or(owner_consumed.map(String::as_str));
                let Some(owner) = owner else {
                    errors.push_for(
                        Some(&parsed[i].identity_path()),
                        CompileError::new(
                            "bynk.event.unknown_subscription",
                            id.span,
                            format!(
                                "`{}` is not a declared event in this context or any consumed context",
                                id.name
                            ),
                        )
                        .with_note(
                            "check the spelling, or add `consumes <context>` for the context whose `event` this names — an unresolvable subscription never receives anything, silently",
                        ),
                    );
                    continue;
                };
                // Events track, slice 1 (spine #936): once the event itself
                // resolves, check the subscription pattern's fields against
                // its declared record shape. No pattern is the pattern-less
                // form (slice 0) and needs none of this.
                let Some(pattern) = pattern else {
                    continue;
                };
                let Some(event_decl) = unit_tables.get(owner).and_then(|t| t.events.get(&id.name))
                else {
                    continue;
                };
                check_event_pattern(
                    pattern,
                    event_decl,
                    owner,
                    unit_tables,
                    unit_uses,
                    &parsed[i].identity_path(),
                    errors,
                );
            }
        }
    }
}

/// Events track, slice 1 (spine #936): resolve a subscription pattern's
/// fields/values against the owning event's declared record shape. `owner`
/// is the context that declares `event_decl` (may differ from the
/// subscribing context, reached via `consumes`) — a field's own type (e.g. a
/// discriminator sum like `Region`) resolves against the *owner's* types
/// (locally declared, or pulled in via the owner's own `uses <commons>`),
/// mirroring how the field's type is resolved everywhere else the event's
/// record shape is used.
fn check_event_pattern(
    pattern: &EventPattern,
    event_decl: &EventDecl,
    owner: &str,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
    identity_path: &std::path::Path,
    errors: &mut ErrorSink,
) {
    let mut seen: HashSet<String> = HashSet::new();
    for field in &pattern.fields {
        if !seen.insert(field.name.name.clone()) {
            errors.push_for(
                Some(identity_path),
                CompileError::new(
                    "bynk.event.pattern_duplicate_field",
                    field.name.span,
                    format!(
                        "field `{}` is matched more than once in this subscription pattern",
                        field.name.name
                    ),
                ),
            );
            continue;
        }
        let Some(record_field) = event_decl
            .body
            .fields
            .iter()
            .find(|f| f.name.name == field.name.name)
        else {
            let known: Vec<&str> = event_decl
                .body
                .fields
                .iter()
                .map(|f| f.name.name.as_str())
                .collect();
            errors.push_for(
                Some(identity_path),
                CompileError::new(
                    "bynk.event.pattern_unknown_field",
                    field.name.span,
                    format!(
                        "`{}` has no field named `{}`",
                        event_decl.name.name, field.name.name
                    ),
                )
                .with_note(format!(
                    "declared fields: {}",
                    if known.is_empty() {
                        "(none)".to_string()
                    } else {
                        known.join(", ")
                    }
                )),
            );
            continue;
        };
        check_event_pattern_value(
            &field.value,
            record_field,
            owner,
            unit_tables,
            unit_uses,
            identity_path,
            errors,
        );
    }
}

/// Events track, slice 4 (spine #936): `via schema(N)`'s `N` must be a
/// positive `Int` literal — the identical rule `@schema(N)` already
/// enforces (`bynk.event.bad_schema_version`), reused under its own code
/// since the two are unrelated syntax positions (an annotation on the
/// event's own declaration vs. a clause on a subscriber's header).
fn check_schema_dispatch(
    dispatch: &SchemaDispatch,
    identity_path: &std::path::Path,
    errors: &mut ErrorSink,
) {
    let SchemaVersionPattern::Literal(n) = &dispatch.pattern;
    if *n <= 0 {
        errors.push_for(
            Some(identity_path),
            CompileError::new(
                "bynk.event.bad_schema_dispatch",
                dispatch.span,
                "`via schema(...)`'s argument must be a positive `Int` literal",
            ),
        );
    }
}

/// Resolve one pattern field's matched value against that field's declared
/// type — a literal must match the field's base type; a variant must name a
/// nullary member of the field's sum type.
fn check_event_pattern_value(
    value: &EventPatternValue,
    record_field: &RecordField,
    owner: &str,
    unit_tables: &HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
    identity_path: &std::path::Path,
    errors: &mut ErrorSink,
) {
    match value {
        EventPatternValue::Literal { value: lit, span } => {
            // A base type (`Int`/`String`/`Bool`/…) is its own `TypeRef`
            // variant, not `TypeRef::Named` — only a *user*-declared type
            // (including a refined/opaque type built on a base) goes through
            // `resolve_type_decl`. An earlier version of this match only
            // handled the `Named` case, so a plain `orderId: String` field
            // (the common case) fell through to "not a literal-kind type",
            // caught by `events_workers_wiring.rs`'s patterned fixture.
            let base = match &record_field.type_ref {
                TypeRef::Base(b, _) => Some(*b),
                TypeRef::Named(field_type_name) => {
                    resolve_type_decl(unit_tables, unit_uses, owner, &field_type_name.name)
                        .and_then(|d| match &d.body {
                            TypeBody::Refined { base, .. } | TypeBody::Opaque { base, .. } => {
                                Some(*base)
                            }
                            _ => None,
                        })
                }
                _ => None,
            };
            let Some(base) = base else {
                errors.push_for(
                    Some(identity_path),
                    CompileError::new(
                        "bynk.event.pattern_type_mismatch",
                        *span,
                        format!(
                            "field `{}` is not a literal-kind type — a literal pattern value cannot match it",
                            record_field.name.name
                        ),
                    ),
                );
                return;
            };
            let kind_matches = matches!(
                (lit, base),
                (LiteralValue::Int(_), BaseType::Int)
                    | (LiteralValue::Str(_), BaseType::String)
                    | (LiteralValue::Bool(_), BaseType::Bool)
            );
            if !kind_matches {
                errors.push_for(
                    Some(identity_path),
                    CompileError::new(
                        "bynk.event.pattern_type_mismatch",
                        *span,
                        format!(
                            "this literal does not match the type of field `{}` (`{}`)",
                            record_field.name.name,
                            type_ref_to_display(&record_field.type_ref)
                        ),
                    ),
                );
            }
        }
        EventPatternValue::Variant {
            type_name,
            variant,
            span,
        } => {
            let TypeRef::Named(field_type_name) = &record_field.type_ref else {
                errors.push_for(
                    Some(identity_path),
                    CompileError::new(
                        "bynk.event.pattern_type_mismatch",
                        *span,
                        format!(
                            "field `{}` is not a sum type — a variant pattern value cannot match it",
                            record_field.name.name
                        ),
                    ),
                );
                return;
            };
            if let Some(qualifier) = type_name
                && qualifier.name != field_type_name.name
            {
                errors.push_for(
                    Some(identity_path),
                    CompileError::new(
                        "bynk.event.pattern_type_mismatch",
                        qualifier.span,
                        format!(
                            "field `{}` has type `{}`, not `{}`",
                            record_field.name.name, field_type_name.name, qualifier.name
                        ),
                    ),
                );
                return;
            }
            let Some(decl) =
                resolve_type_decl(unit_tables, unit_uses, owner, &field_type_name.name)
            else {
                // The field's own type failed to resolve — a different,
                // pre-existing check (ordinary type-reference resolution)
                // already reports this; don't double-report it here.
                return;
            };
            let TypeBody::Sum(sum) = &decl.body else {
                errors.push_for(
                    Some(identity_path),
                    CompileError::new(
                        "bynk.event.pattern_type_mismatch",
                        *span,
                        format!(
                            "field `{}` has type `{}`, which is not a sum type",
                            record_field.name.name, field_type_name.name
                        ),
                    ),
                );
                return;
            };
            let Some(member) = sum.variants.iter().find(|v| v.name.name == variant.name) else {
                errors.push_for(
                    Some(identity_path),
                    CompileError::new(
                        "bynk.event.pattern_unknown_variant",
                        variant.span,
                        format!(
                            "`{}` has no variant named `{}`",
                            field_type_name.name, variant.name
                        ),
                    ),
                );
                return;
            };
            if !member.payload.is_empty() {
                errors.push_for(
                    Some(identity_path),
                    CompileError::new(
                        "bynk.event.pattern_variant_payload",
                        variant.span,
                        format!(
                            "`{}.{}` carries a payload — only a nullary variant may be matched here, since testing the tag alone would silently ignore the payload",
                            field_type_name.name, variant.name
                        ),
                    ),
                );
            }
        }
    }
}

/// Resolve a named type as `owner` sees it: the context's own `types` first,
/// then any commons unit it `uses`. Events track slice 1 (spine #936) needs
/// this because a pattern field's type (e.g. a discriminator sum) may be
/// declared in a commons the event's owning context pulls in with `uses`,
/// rather than in the context itself.
fn resolve_type_decl<'a>(
    unit_tables: &'a HashMap<String, UnitTable>,
    unit_uses: &HashMap<String, Vec<String>>,
    owner: &str,
    name: &str,
) -> Option<&'a Arc<TypeDecl>> {
    if let Some(t) = unit_tables.get(owner).and_then(|t| t.types.get(name)) {
        return Some(t);
    }
    for used in unit_uses.get(owner).into_iter().flatten() {
        if let Some(t) = unit_tables.get(used).and_then(|t| t.types.get(name)) {
            return Some(t);
        }
    }
    None
}

/// message-bundles slice 3 (#878): reports `bynk.messages.malformed_icu_syntax`
/// for every ICU-dispatch placeholder in `entry.template` that fails to
/// parse (unbalanced arm braces, an unknown format keyword, `#` outside a
/// `plural` arm, a missing mandatory `other` arm, or an explicitly
/// out-of-scope construct — `selectordinal`, `offset:`/`=N`, an unrecognised
/// skeleton). Runs once per entry, independent of `@reference` cardinality.
///
/// Decision C: no `MessageEntry` position-map field exists — the span is
/// derived by byte-offset arithmetic against `entry.template_span`, which
/// covers the *raw quoted source token*, while `entry.template` is the
/// *decoded* value (only `\n \t \" \\` are decoded, each shrinking 2 raw
/// bytes to 1 — `bynk-syntax/src/parser.rs`'s `parse_string_literal`). `+ 1`
/// skips the opening quote. This is exact unless an escape occurs *earlier
/// in the same template*, in which case the derived span under-shoots by the
/// number of such escapes — a named, accepted approximation, not a claim of
/// general precision (real message templates essentially never contain an
/// escape before an ICU placeholder).
fn check_entry_icu_syntax(entry: &MessageEntry, file: Option<&Path>, errors: &mut ErrorSink) {
    for (inner_offset, inner) in emitter::icu_dispatch_placeholders(&entry.template) {
        if let Err(e) = emitter::parse_icu_placeholder(inner) {
            let decoded_start = inner_offset + e.offset;
            let decoded_span = Span::new(decoded_start, decoded_start + e.len);
            let raw_span = decoded_span.offset(entry.template_span.start + 1);
            errors.push_for(
                file,
                CompileError::new(
                    "bynk.messages.malformed_icu_syntax",
                    raw_span,
                    e.kind.message(),
                ),
            );
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
