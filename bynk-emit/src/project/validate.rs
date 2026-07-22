use super::*;
use crate::emitter;
use bynk_check::builtin_names::methods::{OF, UNSAFE};

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
    groups: &HashMap<String, Vec<usize>>,
    kinds: &HashMap<String, UnitKind>,
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
/// clause) that a commons declaring `messages` also `uses bynk.locale` —
/// the generated bundle-scoped `render`'s fallback needs it in scope.
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
    groups: &HashMap<String, Vec<usize>>,
    kinds: &HashMap<String, UnitKind>,
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
                        Some(&parsed[i].identity_path),
                        CompileError::new(
                            "bynk.messages.outside_commons",
                            m.span,
                            "`messages` declarations are only allowed inside a commons, not a context or adapter",
                        ),
                    );
                    continue;
                }
                // message-bundles slice 2 (#874, PR #875 review): two blocks
                // declaring the same locale tag are rejected, not
                // last-write-wins — the emitter (`emit_messages_bundle`)
                // has no dedup of its own and would emit two colliding
                // `const __messages_<tag>` declarations, a hard `tsc`
                // redeclare error. Mirrors `bynk.resolve.duplicate_fn`'s own
                // shape: only the *first* occurrence seeds `by_tag`, so a
                // third duplicate still reports against the original, not
                // the second.
                if let Some(&(_, prev)) = by_tag.get(m.tag.name.as_str()) {
                    errors.push_for(
                        Some(&parsed[i].identity_path),
                        CompileError::new(
                            "bynk.resolve.duplicate_message_locale",
                            m.tag.span,
                            format!(
                                "locale \"{}\" is already declared in this bundle",
                                m.tag.name
                            ),
                        )
                        .with_label(prev.tag.span, "previously declared here"),
                    );
                } else {
                    by_tag.insert(m.tag.name.as_str(), (i, m));
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
                            Some(&parsed[i].identity_path),
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
                    check_entry_icu_syntax(entry, Some(&parsed[i].identity_path), errors);
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
                    Some(&parsed[first_i].identity_path),
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
                    if tag == reference.tag.name.as_str() {
                        continue;
                    }
                    for ref_entry in &reference.entries {
                        let Some(locale_entry) =
                            locale_m.entries.iter().find(|e| e.code == ref_entry.code)
                        else {
                            errors.push_for(
                                Some(&parsed[locale_i].identity_path),
                                CompileError::new(
                                    "bynk.messages.incomplete",
                                    locale_m.span,
                                    format!(
                                        "locale \"{tag}\" is missing code \"{}\", declared by the reference locale \"{}\"",
                                        ref_entry.code, reference.tag.name
                                    ),
                                ),
                            );
                            continue;
                        };
                        let ref_names = emitter::placeholder_names(&ref_entry.template);
                        let locale_names = emitter::placeholder_names(&locale_entry.template);
                        if ref_names != locale_names {
                            errors.push_for(
                                Some(&parsed[locale_i].identity_path),
                                CompileError::new(
                                    "bynk.messages.placeholder_mismatch",
                                    locale_entry.template_span,
                                    format!(
                                        "locale \"{tag}\"'s template for code \"{}\" uses placeholders {locale_names:?}, but the reference locale \"{}\"'s uses {ref_names:?}",
                                        ref_entry.code, reference.tag.name
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
                                    Some(&parsed[locale_i].identity_path),
                                    CompileError::new(
                                        "bynk.messages.format_mismatch",
                                        locale_entry.template_span,
                                        format!(
                                            "locale \"{tag}\"'s placeholder \"{pname}\" in code \"{}\" is formatted as {}, but the reference locale \"{}\"'s is {}",
                                            ref_entry.code,
                                            locale_kind.as_str(),
                                            reference.tag.name,
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
                        Some(&parsed[i].identity_path),
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
        let has_locale_uses = unit_uses
            .get(name)
            .is_some_and(|targets| targets.iter().any(|t| t == "bynk.locale"));
        if !has_locale_uses {
            errors.push_for(
                Some(&parsed[first_i].identity_path),
                CompileError::new(
                    "bynk.messages.missing_locale_dependency",
                    first_span,
                    "a commons declaring `messages` must also `uses bynk.locale`",
                ),
            );
        }
    }
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

/// Enforce v0.4 construction rules: types owned by a consumed context can be
/// referenced (held, passed, read for transparent exports) but cannot be
/// constructed. This catches `OtherType { ... }`, `OtherType.of(...)`,
/// `OtherType.unsafe(...)`, and `OtherType.Variant(...)` expressions where
/// `OtherType` is from a consumed context.
pub(crate) fn check_context_constraints(
    typed: &checker::TypedCommons,
    consumed_types: &HashMap<String, ConsumedType>,
    local_type_names: &HashSet<String>,
) -> Vec<CompileError> {
    let mut errors = Vec::new();
    for item in &typed.commons.items {
        if let CommonsItem::Fn(f) = item {
            walk_block_for_constraints(
                &f.body,
                typed,
                consumed_types,
                local_type_names,
                &mut errors,
            );
        }
    }
    errors
}

fn walk_block_for_constraints(
    block: &Block,
    typed: &checker::TypedCommons,
    consumed: &HashMap<String, ConsumedType>,
    local: &HashSet<String>,
    errors: &mut Vec<CompileError>,
) {
    for stmt in &block.statements {
        match stmt {
            Statement::Let(l) | Statement::EffectLet(l) => {
                walk_expr_for_constraints(&l.value, typed, consumed, local, errors);
            }
            Statement::Expect(a) => {
                walk_expr_for_constraints(&a.value, typed, consumed, local, errors);
            }
            Statement::Send(s) => {
                walk_expr_for_constraints(&s.value, typed, consumed, local, errors);
            }
            Statement::Do(d) => {
                walk_expr_for_constraints(&d.value, typed, consumed, local, errors);
            }
            Statement::Assign(a) => {
                walk_expr_for_constraints(&a.value, typed, consumed, local, errors);
            }
        }
    }
    walk_expr_for_constraints(&block.tail, typed, consumed, local, errors);
}

fn walk_expr_for_constraints(
    e: &Expr,
    typed: &checker::TypedCommons,
    consumed: &HashMap<String, ConsumedType>,
    local: &HashSet<String>,
    errors: &mut Vec<CompileError>,
) {
    match &e.kind {
        ExprKind::ListLit(elems) => {
            for el in elems {
                walk_expr_for_constraints(el, typed, consumed, local, errors);
            }
        }
        ExprKind::Wire(inner) => {
            walk_expr_for_constraints(inner, typed, consumed, local, errors);
        }
        // v0.43: a hole's expression is checked like any other.
        ExprKind::InterpStr(parts) => {
            for part in parts {
                if let InterpPart::Hole(hole) = part {
                    walk_expr_for_constraints(hole, typed, consumed, local, errors);
                }
            }
        }
        ExprKind::RecordConstruction { type_name, fields } => {
            if let Some(ct) = consumed.get(&type_name.name) {
                errors.push(
                    CompileError::new(
                        "bynk.context.external_construction",
                        type_name.span,
                        format!(
                            "cannot construct `{}` here — it is owned by context `{}`",
                            type_name.name, ct.owning_context,
                        ),
                    )
                    .with_note(
                        "values of an externally-owned type can only be created inside the owning context",
                    ),
                );
            }
            for f in fields {
                if let Some(v) = &f.value {
                    walk_expr_for_constraints(v, typed, consumed, local, errors);
                }
            }
        }
        ExprKind::ConstructorCall {
            type_name,
            method,
            args,
        } => {
            if let Some(ct) = consumed.get(&type_name.name) {
                let is_construct = method.name == OF
                    || method.name == UNSAFE
                    || matches!(
                        typed.types.get(&type_name.name).map(|d| &d.body),
                        Some(TypeBody::Sum(s)) if s.variants.iter().any(|v| v.name.name == method.name),
                    );
                if is_construct {
                    errors.push(
                        CompileError::new(
                            "bynk.context.external_construction",
                            type_name.span.merge(method.span),
                            format!(
                                "cannot construct `{}.{}` here — `{}` is owned by context `{}`",
                                type_name.name, method.name, type_name.name, ct.owning_context,
                            ),
                        )
                        .with_note(
                            "values of an externally-owned type can only be created inside the owning context",
                        ),
                    );
                }
            }
            for a in args {
                walk_expr_for_constraints(a, typed, consumed, local, errors);
            }
        }
        ExprKind::MethodCall {
            receiver,
            method,
            args,
            ..
        } => {
            // `T.method(...)` written as MethodCall with receiver Ident(T).
            if let ExprKind::Ident(id) = &receiver.kind
                && let Some(ct) = consumed.get(&id.name)
            {
                let is_construct = method.name == OF
                    || method.name == UNSAFE
                    || matches!(
                        typed.types.get(&id.name).map(|d| &d.body),
                        Some(TypeBody::Sum(s)) if s.variants.iter().any(|v| v.name.name == method.name),
                    );
                if is_construct {
                    errors.push(
                        CompileError::new(
                            "bynk.context.external_construction",
                            id.span.merge(method.span),
                            format!(
                                "cannot construct `{}.{}` here — `{}` is owned by context `{}`",
                                id.name, method.name, id.name, ct.owning_context,
                            ),
                        )
                        .with_note(
                            "values of an externally-owned type can only be created inside the owning context",
                        ),
                    );
                }
            }
            walk_expr_for_constraints(receiver, typed, consumed, local, errors);
            for a in args {
                walk_expr_for_constraints(a, typed, consumed, local, errors);
            }
        }
        ExprKind::FieldAccess { receiver, field } => {
            // For opaque-exported types from consumed contexts, field
            // access is forbidden — but record types have field access
            // anyway, so the visibility check applies only when the
            // receiver's type is a consumed type. To do this rigorously,
            // we'd consult the expr_types map. Easy path: peek at the
            // receiver if it's an Ident referring to a binding whose
            // declared type points to a consumed type.
            // For v0.4 we use a simpler conservative rule: if the
            // receiver is `T.X` syntax (FieldAccess from an Ident that's
            // a type name) and `T` is consumed and opaque, reject it.
            if let ExprKind::Ident(id) = &receiver.kind
                && let Some(ct) = consumed.get(&id.name)
                && ct.visibility == Visibility::Opaque
                && typed
                    .types
                    .get(&id.name)
                    .map(|d| matches!(d.body, TypeBody::Sum(_)))
                    .unwrap_or(false)
            {
                errors.push(
                    CompileError::new(
                        "bynk.context.opaque_inspection",
                        id.span.merge(field.span),
                        format!(
                            "cannot inspect opaquely-exported type `{}` from outside context `{}`",
                            id.name, ct.owning_context,
                        ),
                    )
                    .with_note(
                        "opaque exports hide the type's shape; the owning context did not expose variants or fields",
                    ),
                );
            }
            walk_expr_for_constraints(receiver, typed, consumed, local, errors);
        }
        ExprKind::Match { discriminant, arms } => {
            // If the discriminant is typed as an opaquely-exported consumed
            // type, the match is forbidden because we can't reveal the
            // variants.
            if let Some(ty) = typed.expr_types.get(&discriminant.span) {
                let display = ty.display();
                if let Some(ct) = consumed.get(&display)
                    && ct.visibility == Visibility::Opaque
                {
                    errors.push(
                        CompileError::new(
                            "bynk.context.opaque_inspection",
                            discriminant.span,
                            format!(
                                "cannot `match` on opaquely-exported type `{}` from outside context `{}`",
                                display, ct.owning_context,
                            ),
                        )
                        .with_note(
                            "opaque exports hide the type's shape; the owning context did not expose variants",
                        ),
                    );
                }
            }
            walk_expr_for_constraints(discriminant, typed, consumed, local, errors);
            for arm in arms {
                match &arm.body {
                    MatchBody::Expr(ex) => {
                        walk_expr_for_constraints(ex, typed, consumed, local, errors);
                    }
                    MatchBody::Block(b) => {
                        walk_block_for_constraints(b, typed, consumed, local, errors);
                    }
                }
            }
        }
        ExprKind::Is { value, pattern: _ } => {
            walk_expr_for_constraints(value, typed, consumed, local, errors);
        }
        ExprKind::Call { args, .. } => {
            for a in args {
                walk_expr_for_constraints(a, typed, consumed, local, errors);
            }
        }
        ExprKind::BinOp(_, l, r) => {
            walk_expr_for_constraints(l, typed, consumed, local, errors);
            walk_expr_for_constraints(r, typed, consumed, local, errors);
        }
        ExprKind::UnaryOp(_, i)
        | ExprKind::Paren(i)
        | ExprKind::Ok(i)
        | ExprKind::Err(i)
        | ExprKind::Some(i)
        | ExprKind::Question(i) => {
            walk_expr_for_constraints(i, typed, consumed, local, errors);
        }
        // v0.20a: walk a lambda's body for construction constraints.
        ExprKind::Lambda(lambda) => {
            walk_expr_for_constraints(&lambda.body, typed, consumed, local, errors)
        }
        ExprKind::Block(b) => walk_block_for_constraints(b, typed, consumed, local, errors),
        ExprKind::If {
            cond,
            then_block,
            else_block,
        } => {
            walk_expr_for_constraints(cond, typed, consumed, local, errors);
            walk_block_for_constraints(then_block, typed, consumed, local, errors);
            walk_block_for_constraints(else_block, typed, consumed, local, errors);
        }
        ExprKind::Ident(_)
        | ExprKind::IntLit { .. }
        | ExprKind::FloatLit { .. }
        | ExprKind::DurationLit { .. }
        | ExprKind::StrLit(_)
        | ExprKind::BoolLit(_)
        | ExprKind::None
        | ExprKind::UnitLit => {}
        ExprKind::EffectPure(inner) => {
            walk_expr_for_constraints(inner, typed, consumed, local, errors);
        }
        ExprKind::Expect(inner) => {
            walk_expr_for_constraints(inner, typed, consumed, local, errors);
        }
        ExprKind::Val { args, .. } => {
            for a in args {
                walk_expr_for_constraints(a, typed, consumed, local, errors);
            }
        }
        // v0.117: observation/`trace` are test-only; constraint checks run over
        // production code, but walk the predicate defensively for completeness.
        ExprKind::Observation(o) => {
            if let ObservationMatcher::Called { count, with_pred } = &o.matcher {
                if let Some(c) = count {
                    walk_expr_for_constraints(c, typed, consumed, local, errors);
                }
                if let Some(p) = with_pred {
                    walk_expr_for_constraints(p, typed, consumed, local, errors);
                }
            }
        }
        ExprKind::Trace { .. } => {}
        ExprKind::RecordSpread {
            base, overrides, ..
        } => {
            walk_expr_for_constraints(base, typed, consumed, local, errors);
            for f in overrides {
                if let Some(v) = &f.value {
                    walk_expr_for_constraints(v, typed, consumed, local, errors);
                }
            }
        }
    }
}

/// Check capability/provider/service/agent declaration bodies for a context (or
/// adapter) unit. Mutates `typed` to extend the expr_types map with bindings
/// observed in the new bodies.
///
/// The parent builds the shared state read by every per-kind validator — a
/// `resolved` commons snapshot and the `capability_info_map` (local capability
/// signatures, extended with the cross-context flattened caps) — then runs the
/// per-declaration-kind validators in a fixed order. The order is load-bearing:
/// multi-error fixtures assert the diagnostic sequence
/// (capabilities → providers → services → agents).
pub(crate) fn check_context_declarations(
    typed: &mut checker::TypedCommons,
    table: &UnitTable,
    cross_context: &resolver::CrossContextInfo,
    refs: &mut RefSink,
    hints: &mut HintSink,
    locals: &mut LocalsSink,
    requirements: &mut RequirementSink,
) -> Vec<CompileError> {
    let mut errors = Vec::new();
    let no_vars: HashSet<String> = HashSet::new();

    // Build a resolved-commons snapshot for the per-handler checker.
    // We synthesise a ResolvedCommons by reusing typed.types / typed.fns /
    // typed.methods; the resolver wouldn't add anything new.
    let local_type_names: std::collections::HashSet<String> = typed.types.keys().cloned().collect();
    let resolved = ResolvedCommons {
        commons: typed.commons.clone(),
        types: typed.types.clone(),
        fns: typed.fns.clone(),
        methods: typed.methods.clone(),
        local_type_names,
        cross_context: cross_context.clone(),
        agents: table.agents.clone(),
        imported_from: HashMap::new(),
    };

    // v0.25: capability operation signatures reference types.
    check_capability_decls(table, &typed.types, &no_vars, refs);

    // Capability info from the table.
    let mut capability_info_map: HashMap<String, CapabilityInfo> = table
        .capabilities
        .iter()
        .map(|(name, decl)| {
            let ops = decl
                .ops
                .iter()
                .map(|op| CapabilityOpInfo {
                    name: op.name.name.clone(),
                    params: op
                        .params
                        .iter()
                        .map(|p| checker::resolve_type_ref(&p.type_ref, &typed.types))
                        .map(|t| t.unwrap_or(Ty::Unit))
                        .collect(),
                    param_names: op.params.iter().map(|p| p.name.name.clone()).collect(),
                    return_ty: checker::resolve_type_ref(&op.return_type, &typed.types)
                        .unwrap_or(Ty::Unit),
                })
                .collect();
            (
                name.clone(),
                CapabilityInfo {
                    name: name.clone(),
                    ops,
                },
            )
        })
        .collect();
    // v0.17: flattened capabilities (`consumes U { Cap }`) enter the local map
    // under their bare names, resolved from the consumed unit's exported
    // capability so bare `given Cap` / `Cap.op(…)` type-check as if local.
    for (cap, unit) in &cross_context.flattened_caps {
        let Some(xcap) = cross_context
            .consumed_capabilities
            .get(unit)
            .and_then(|m| m.get(cap))
        else {
            continue;
        };
        let ops = xcap
            .ops
            .iter()
            .map(|op| CapabilityOpInfo {
                name: op.name.clone(),
                params: op
                    .params
                    .iter()
                    .map(|(_, tr)| checker::resolve_type_ref(tr, &typed.types).unwrap_or(Ty::Unit))
                    .collect(),
                param_names: op.params.iter().map(|(n, _)| n.clone()).collect(),
                return_ty: checker::resolve_type_ref(&op.return_type, &typed.types)
                    .unwrap_or(Ty::Unit),
            })
            .collect();
        capability_info_map.insert(
            cap.clone(),
            CapabilityInfo {
                name: cap.clone(),
                ops,
            },
        );
    }

    check_provider_decls(
        typed,
        table,
        cross_context,
        &resolved,
        &capability_info_map,
        refs,
        hints,
        locals,
        requirements,
        &mut errors,
    );
    check_service_decls(
        typed,
        table,
        cross_context,
        &resolved,
        &capability_info_map,
        refs,
        hints,
        locals,
        requirements,
        &mut errors,
    );
    check_agent_decls(
        typed,
        table,
        cross_context,
        &capability_info_map,
        &no_vars,
        refs,
        hints,
        locals,
        requirements,
        &mut errors,
    );

    errors
}

/// v0.25: capability operation signatures reference types; record them under
/// the capability as owner (the table is unit-level — the owner re-attributes
/// spans to the declaring file at assembly).
fn check_capability_decls(
    table: &UnitTable,
    types: &HashMap<String, TypeDecl>,
    no_vars: &HashSet<String>,
    refs: &mut RefSink,
) {
    for (name, decl) in &table.capabilities {
        refs.set_owner(name);
        for op in &decl.ops {
            for p in &op.params {
                checker::record_type_refs(&p.type_ref, types, no_vars, refs);
            }
            checker::record_type_refs(&op.return_type, types, no_vars, refs);
        }
    }
    refs.clear_owner();
}

/// Check provider bodies. v0.12: a provider may declare `given` and use
/// those capabilities in its bodies (provider composition). Bodies are
/// effectful if the operation returns Effect[T]; no `self`. Also detects
/// provider dependency cycles over capabilities.
#[allow(clippy::too_many_arguments)]
fn check_provider_decls(
    typed: &mut checker::TypedCommons,
    table: &UnitTable,
    cross_context: &resolver::CrossContextInfo,
    resolved: &ResolvedCommons,
    capability_info_map: &HashMap<String, CapabilityInfo>,
    refs: &mut RefSink,
    hints: &mut HintSink,
    locals: &mut LocalsSink,
    requirements: &mut RequirementSink,
    errors: &mut Vec<CompileError>,
) {
    for provider in table.providers.values() {
        refs.set_owner(&provider.provider_name.name);
        // v0.25: `provides Cap = …` references the capability.
        // v0.35 (ADR 0068): and records a capability→provider implementation edge.
        if table.capabilities.contains_key(&provider.capability.name)
            || cross_context
                .flattened_caps
                .contains_key(&provider.capability.name)
        {
            record_provides_clause_ref(&provider.capability, cross_context, refs);
        }
        // Build the provider's capability scope from its `given`, validating
        // each name is a declared capability.
        let mut provider_caps: HashMap<String, CapabilityInfo> = HashMap::new();
        for cap_ref in &provider.given {
            if let Some(info) =
                resolve_given_cap_ref(cap_ref, capability_info_map, cross_context, errors, refs)
            {
                provider_caps.insert(cap_ref.key().to_string(), info);
            }
        }
        for op in &provider.ops {
            // The provider's `given` keys are in scope (so cross-context
            // capability calls resolve), but unused-`given` is not reported
            // per-op: a capability may be used in one op but not another.
            // No `given_anchor`: the clause lives on the `provides` line,
            // not at the op's return type, so an absent clause is not
            // synthesised here (v0.26).
            checker::check_handler_body(
                resolved,
                checker::HandlerBodyCheck {
                    capabilities: provider_caps.clone(),
                    declared_capabilities: capability_info_map.clone(),
                    ..checker::HandlerBodyCheck::new(
                        &op.body,
                        &op.return_type,
                        &op.params,
                        &provider.given,
                    )
                },
                checker::CheckSinks {
                    expr_types: &mut typed.expr_types,
                    errors,
                    refs,
                    hints,
                    locals,
                    requirements,
                },
            );
        }
    }

    // v0.12: providers form a dependency graph over capabilities (a provider's
    // `given` are the capabilities its provided capability depends on). Reject
    // a cycle — the composition root cannot instantiate one in dependency
    // order. Self-provision (`provides X = … given X`) is the trivial cycle.
    detect_provider_dependency_cycles(&table.providers, errors);
}

/// Check service handlers across all services in this context: HTTP/cron/queue
/// handler shape and per-kind duplicate detection (route/schedule/consumer),
/// then each handler's `given` clause and body. The duplicate-detection passes
/// run before the body pass so the `bynk.<kind>.duplicate_*` diagnostics
/// precede the body diagnostics in multi-error fixtures.
/// v0.44: a service is one protocol adapter — every handler's form must match
/// the `from <protocol>` header. A `from`-less service (`Call`) admits only
/// `on call`; mismatches are `bynk.service.{missing_from,mixed_protocols}`.
fn check_service_protocols(table: &UnitTable, errors: &mut Vec<CompileError>) {
    // v0.104 (slice 3b, D5): at v1 the Workers upgrade routes by the `Upgrade:
    // websocket` header alone (no path/query discriminator), so a context may hold
    // at most one `from websocket` service. Report every WS service past the first
    // (name-sorted for a deterministic diagnostic).
    let mut ws_services: Vec<&ServiceDecl> = table
        .services
        .values()
        .filter(|s| matches!(s.protocol, ServiceProtocol::WebSocket { .. }))
        .collect();
    ws_services.sort_by(|a, b| a.name.name.cmp(&b.name.name));
    for extra in ws_services.iter().skip(1) {
        errors.push(
            CompileError::new(
                "bynk.service.websocket_multiple",
                extra.name.span,
                format!(
                    "this context holds more than one `from websocket` service (`{}`) — at v1 the upgrade routes by the `Upgrade: websocket` header alone, so a context may host only one",
                    extra.name.name
                ),
            )
            .with_note("split the WebSocket services into separate contexts; per-path routing of multiple WebSocket endpoints is a named follow-on"),
        );
    }
    for service in table.services.values() {
        // v0.103: a `from websocket` service holds exactly one `on open` handler
        // (the edge upgrade); inbound frames are the agent's typed messages, not
        // service handlers.
        if matches!(service.protocol, ServiceProtocol::WebSocket { .. }) {
            let opens: Vec<&Handler> = service
                .handlers
                .iter()
                .filter(|h| matches!(h.kind, HandlerKind::Open))
                .collect();
            if opens.is_empty() {
                errors.push(
                    CompileError::new(
                        "bynk.service.websocket_open_arity",
                        service.name.span,
                        format!(
                            "the `from websocket` service `{}` has no `on open` handler — it needs exactly one (the edge upgrade)",
                            service.name.name
                        ),
                    )
                    .with_note("a `from websocket` service holds exactly one `on open`, and optionally one `on message` (inbound) and one `on close`"),
                );
            } else if opens.len() > 1 {
                errors.push(CompileError::new(
                    "bynk.service.websocket_open_arity",
                    opens[1].span,
                    format!(
                        "the `from websocket` service `{}` has more than one `on open` handler — it needs exactly one",
                        service.name.name
                    ),
                ));
            }
            // v0.106 (slice 3b-iii): the inbound `on message` and `on close` are
            // optional but at most one each; an `on message` carries the decoded
            // inbound frame as the single param typed as the service's `in` type.
            let ServiceProtocol::WebSocket { in_type, .. } = &service.protocol else {
                unreachable!("guarded by the enclosing match");
            };
            let messages: Vec<&Handler> = service
                .handlers
                .iter()
                .filter(|h| matches!(h.kind, HandlerKind::Message))
                .collect();
            let closes: Vec<&Handler> = service
                .handlers
                .iter()
                .filter(|h| matches!(h.kind, HandlerKind::Close))
                .collect();
            if messages.len() > 1 {
                errors.push(CompileError::new(
                    "bynk.service.websocket_open_arity",
                    messages[1].span,
                    format!(
                        "the `from websocket` service `{}` has more than one `on message` handler — it needs at most one",
                        service.name.name
                    ),
                ));
            }
            if closes.len() > 1 {
                errors.push(CompileError::new(
                    "bynk.service.websocket_open_arity",
                    closes[1].span,
                    format!(
                        "the `from websocket` service `{}` has more than one `on close` handler — it needs at most one",
                        service.name.name
                    ),
                ));
            }
            for message in &messages {
                let frame_params = message
                    .params
                    .iter()
                    .filter(|p| type_refs_match(&p.type_ref, in_type))
                    .count();
                if frame_params != 1 {
                    errors.push(
                        CompileError::new(
                            "bynk.ws.message_frame_param",
                            message.span,
                            format!(
                                "a WebSocket `on message` handler must have exactly one parameter of the service's inbound frame type `{}` (the decoded frame), but found {frame_params}",
                                ts_type_ref_display(in_type)
                            ),
                        )
                        .with_note(
                            "declare the frame as a parameter, e.g. `on message by user: Actor (frame: ClientFrame)`; any other parameters are route values recovered from the connection",
                        ),
                    );
                }
            }
            // v0.106 (slice 3b-iii): an `on message`/`on close` recovers its
            // non-frame (route) parameters **positionally** from the socket
            // attachment the `on open` accept wrote — so they must be a
            // type-compatible prefix of the `on open` parameters. A mismatch would
            // silently `as`-cast one route value to another's type at the dispatch.
            if let [open] = opens.as_slice() {
                let op = &open.params;
                let route_mismatch = |p: &Param, errors: &mut Vec<CompileError>| {
                    errors.push(
                        CompileError::new(
                            "bynk.ws.route_param_mismatch",
                            p.span,
                            format!(
                                "the route parameter `{}: {}` does not match the `on open` parameter at this position — `on message`/`on close` route values are recovered positionally from the connection, so they must be a type-compatible prefix of the `on open` parameters",
                                p.name.name,
                                ts_type_ref_display(&p.type_ref)
                            ),
                        )
                        .with_note(
                            "give the inbound/close handler the same leading parameters (name aside) as `on open`, in the same order",
                        ),
                    );
                };
                if let [message] = messages.as_slice() {
                    let mut idx = 0usize;
                    for p in &message.params {
                        if type_refs_match(&p.type_ref, in_type) {
                            continue; // the decoded frame, not a route value
                        }
                        if !op
                            .get(idx)
                            .is_some_and(|o| type_refs_match(&p.type_ref, &o.type_ref))
                        {
                            route_mismatch(p, errors);
                        }
                        idx += 1;
                    }
                }
                if let [close] = closes.as_slice() {
                    for (i, p) in close.params.iter().enumerate() {
                        if !op
                            .get(i)
                            .is_some_and(|o| type_refs_match(&p.type_ref, &o.type_ref))
                        {
                            route_mismatch(p, errors);
                        }
                    }
                }
            }
            // v0.104 (D2): on Workers the upgrade is routed to the Durable Object
            // that hosts the connection — the agent the `on open` transfers it to.
            // That target must be statically resolvable: exactly one top-level
            // transfer (`Agent(key).method(…, connection)`).
            let local_agents: std::collections::HashSet<String> =
                table.agents.keys().cloned().collect();
            for open in &opens {
                // v0.104 (slice 3b): an `on open` cannot `given` capabilities — on
                // Workers it runs inside the connection-hosting Durable Object, which
                // has no composition root to supply them (the capabilities belong on
                // the agent handler the connection transfers to).
                if !open.given.is_empty() {
                    errors.push(
                        CompileError::new(
                            "bynk.ws.open_given_unsupported",
                            open.span,
                            "a WebSocket `on open` handler cannot declare `given` capabilities — on Workers it runs inside the connection-hosting Durable Object, which has no composition root to supply them",
                        )
                        .with_note(
                            "move capability use into the agent handler the connection transfers to (it carries its own `given`)",
                        ),
                    );
                }
                use crate::emitter::websocket::{WsOpenShape, analyse_open_shape};
                match analyse_open_shape(&open.body, &local_agents) {
                    WsOpenShape::One(_) => {}
                    WsOpenShape::None => errors.push(
                        CompileError::new(
                            "bynk.ws.open_transfer_shape",
                            open.span,
                            "a WebSocket `on open` handler must transfer its `connection` into exactly one agent — e.g. `Room(roomId).join(…, connection)` — so the upgrade can be routed to the hosting Durable Object",
                        )
                        .with_note(
                            "transfer the connection to an agent unconditionally (not inside an `if`/`match`); a key derivable from a handler parameter routes the upgrade",
                        ),
                    ),
                    WsOpenShape::Multiple => errors.push(CompileError::new(
                        "bynk.ws.open_transfer_shape",
                        open.span,
                        "a WebSocket `on open` handler transfers its `connection` into more than one agent — the upgrade has no single Durable Object to route to",
                    )),
                }
            }
        }
        for handler in &service.handlers {
            let matches_protocol = matches!(
                (&service.protocol, &handler.kind),
                (ServiceProtocol::Call, HandlerKind::Call)
                    | (ServiceProtocol::Http, HandlerKind::Http { .. })
                    | (ServiceProtocol::Cron, HandlerKind::Cron { .. })
                    | (ServiceProtocol::Queue { .. }, HandlerKind::Message)
                    // v0.103/v0.106: a `from websocket` admits `on open` (the
                    // upgrade), and the inbound/close lifecycle `on message`/`on
                    // close` (slice 3b-iii).
                    | (
                        ServiceProtocol::WebSocket { .. },
                        HandlerKind::Open | HandlerKind::Message | HandlerKind::Close
                    )
            );
            if matches_protocol {
                continue;
            }
            match &service.protocol {
                ServiceProtocol::Call => {
                    let suggested = match &handler.kind {
                        HandlerKind::Http { .. } => "from http",
                        HandlerKind::Cron { .. } => "from cron",
                        HandlerKind::Message => "from queue(\"…\")",
                        HandlerKind::Open | HandlerKind::Close => "from websocket(in: …, out: …)",
                        HandlerKind::Call => continue,
                    };
                    errors.push(
                        CompileError::new(
                            "bynk.service.missing_from",
                            handler.span,
                            format!(
                                "this handler needs a protocol on the service header — add `{suggested}` to `service {}`",
                                service.name.name,
                            ),
                        )
                        .with_note("a service with no `from` clause admits only `on call` handlers"),
                    );
                }
                wire => {
                    errors.push(
                        CompileError::new(
                            "bynk.service.mixed_protocols",
                            handler.span,
                            format!(
                                "a `{}` service admits only its own handler form; this handler does not match",
                                protocol_label(wire),
                            ),
                        )
                        .with_note(
                            "a service is one protocol adapter — split differing handlers into separate services",
                        ),
                    );
                }
            }
        }
    }
}

fn protocol_label(p: &ServiceProtocol) -> &'static str {
    match p {
        ServiceProtocol::Call => "call",
        ServiceProtocol::Http => "from http",
        ServiceProtocol::Cron => "from cron",
        ServiceProtocol::Queue { .. } => "from queue",
        ServiceProtocol::WebSocket { .. } => "from websocket",
    }
}

/// v0.45: actor-contract well-formedness and the handler `by`-clause checks.
///
/// Two passes: (1) each `actor` declaration is well-formed — the refinement
/// form's predicate is restricted to the closed actor-claim catalogue over a
/// `Bearer` base, the scheme is admitted, and a declared identity is a
/// context-ownable (sealed) type; (2) each service handler either
/// names an admissible actor on `by` or inherits the protocol default — and
/// HTTP requires an explicit `by`.
/// Validate one `by` clause's actor contracts against a protocol (v0.155,
/// factored so both a handler's own clause and a service-level default are
/// checked by the same logic). `params` is the enclosing handler's parameters —
/// `Some` for a real handler, `None` for a service-level default validated in
/// isolation (a default has no handler body, so the two body-shaped checks —
/// binder/parameter collision and `Signature`-requires-`body` — are skipped for
/// it and re-run per handler when the default is inherited).
fn check_by_clause_contracts(
    by: &bynk_syntax::ast::ByClause,
    params: Option<&[bynk_syntax::ast::Param]>,
    protocol: &ServiceProtocol,
    table: &UnitTable,
    refs: &mut RefSink,
    errors: &mut Vec<CompileError>,
) {
    use bynk_check::actors::{self, Scheme};

    // A named binder introduces a new binding; it must not collide with a handler
    // parameter of the same name (which it would otherwise silently shadow in the
    // body scope). The binder-less form captures nothing, so it can't collide.
    // Only meaningful for a real handler (a default is validated without params).
    if let (Some(params), Some(binder)) = (params, &by.binder)
        && params.iter().any(|p| p.name.name == binder.name)
    {
        errors.push(
            CompileError::new(
                "bynk.actor.binder_shadows_param",
                binder.span,
                format!(
                    "the actor binder `{}` collides with a handler parameter of the same name",
                    binder.name,
                ),
            )
            .with_note("rename the `by` binder or the parameter"),
        );
    }
    // v0.52: a multi-actor sum (`by who: A | B`) must bind the resolved actor —
    // the body learns *which* peer verified by matching on the binder.
    if by.is_sum() && by.binder.is_none() {
        errors.push(
            CompileError::new(
                "bynk.actor.sum_requires_binder",
                by.span,
                "a multi-actor `by` clause must bind the resolved actor",
            )
            .with_note("write `by who: A | B (…)` and `match who { … }` in the body"),
        );
    }
    // Resolve each member to its contract: a local declaration *or* a prelude
    // actor. A local declaration that exists but is malformed (its scheme already
    // errored at the decl) does NOT fall through to a prelude actor of the same
    // name — only an unresolved name is. `members` keeps the resolved peers in
    // declared order for the reachability check below.
    let mut members: Vec<(&bynk_syntax::ast::Ident, actors::Contract)> = Vec::new();
    for actor_ref in &by.actors {
        let local = table.actors.get(&actor_ref.name);
        // A refinement actor (`actor A = B where …`) is never a peer: every `A`
        // is a `B`, so the arm is dead (Q3/Q4).
        if by.is_sum() && local.is_some_and(|a| a.refinement.is_some()) {
            errors.push(
                CompileError::new(
                    "bynk.actor.refinement_in_sum",
                    actor_ref.span,
                    format!(
                        "the refinement actor `{}` cannot be a peer in a multi-actor sum",
                        actor_ref.name
                    ),
                )
                .with_note(
                    "a refinement narrows a base actor — match it inside the \
                     resolved arm, not as a sum member",
                ),
            );
            continue;
        }
        let contract = if let Some(a) = local {
            refs.record(actor_ref.span, SymbolKind::Actor, &actor_ref.name);
            // v0.53: a refinement actor's contract is its base's scheme
            // (refinement elimination — an `Admin` is-a `User`); the invariant
            // rides the seam, not the scheme. A malformed refinement already
            // errored at its decl (pass 1).
            let scheme_actor = match &a.refinement {
                Some(r) => table.actors.get(&r.base.name),
                None => Some(a),
            };
            scheme_actor
                .and_then(|sa| sa.auth.as_ref())
                .and_then(|au| Scheme::from_name(&au.name))
                .filter(|s| s.admitted())
                .map(|scheme| actors::Contract {
                    scheme,
                    identity: actors::Identity::Unit,
                })
        } else {
            actors::prelude_actor(&actor_ref.name)
        };
        let Some(contract) = contract else {
            if local.is_none() {
                errors.push(
                    CompileError::new(
                        "bynk.actor.unknown_actor",
                        actor_ref.span,
                        format!("unknown actor `{}`", actor_ref.name),
                    )
                    .with_note(
                        "name a declared `actor` or a prelude actor \
                         (`Visitor`, `Scheduler`, `Producer`, `Caller`)",
                    ),
                );
            }
            continue;
        };
        if !actors::scheme_admissible(protocol, contract.scheme) {
            errors.push(
                CompileError::new(
                    "bynk.actor.scheme_not_admissible",
                    by.span,
                    format!(
                        "a `{}` actor is not admissible on a `{}` handler",
                        contract.scheme.as_str(),
                        protocol_label(protocol),
                    ),
                )
                .with_note(match protocol {
                    ServiceProtocol::Http => {
                        "public HTTP routes take an anonymous actor — write `by v: Visitor`"
                    }
                    _ => "internal protocols (call/cron/queue) take an `Internal` actor",
                }),
            );
        }
        // v0.54: the `Caller` prelude actor yields a `CallerId` (the calling
        // context's name), a cross-context `on call` concept — it is admissible
        // only on the `Call` protocol, even though its `Internal` scheme is
        // otherwise valid on cron/queue (those take `Scheduler`/`Producer`).
        let is_caller = !table.actors.contains_key(&actor_ref.name)
            && actors::prelude_actor(&actor_ref.name).map(|c| c.identity)
                == Some(actors::Identity::CallerId);
        if is_caller && !matches!(protocol, ServiceProtocol::Call) {
            errors.push(
                CompileError::new(
                    "bynk.actor.scheme_not_admissible",
                    by.span,
                    format!(
                        "the `Caller` actor is not admissible on a `{}` handler",
                        protocol_label(protocol),
                    ),
                )
                .with_note(
                    "`Caller` carries the calling context's identity — it is only \
                     admissible on `on call`; cron takes `Scheduler`, queue takes `Producer`",
                ),
            );
        }
        // v0.151: `Oidc` is single-actor only this slice — a multi-actor sum owns
        // the whole boundary and reads the body once, a shape the OIDC seam (JWKS
        // fetch + async key import) does not yet fit. Reject it as a peer.
        if by.is_sum() && contract.scheme == actors::Scheme::Oidc {
            errors.push(
                CompileError::new(
                    "bynk.actor.oidc_not_in_sum",
                    actor_ref.span,
                    format!(
                        "the `Oidc` actor `{}` cannot be a peer in a multi-actor sum",
                        actor_ref.name
                    ),
                )
                .with_note(
                    "OIDC is single-actor this slice — give the route a single \
                     `by user: <OidcActor>` clause",
                ),
            );
        }
        members.push((actor_ref, contract));
    }
    // v0.51: a Signature member verifies an HMAC over the body, so the handler
    // MUST take a `body` parameter (single or sum). Skipped for a service-level
    // default (no handler body); re-checked per handler when inherited.
    if let Some(params) = params
        && members
            .iter()
            .any(|(_, c)| c.scheme == actors::Scheme::Signature)
        && !params.iter().any(|p| p.name.name == "body")
    {
        errors.push(
            CompileError::new(
                "bynk.actor.signature_requires_body",
                by.span,
                "a `Signature` handler must take a `body` parameter (the signature is over the body)",
            )
            .with_note("add a `(body: T)` parameter to the handler"),
        );
    }
    // v0.52: sum reachability — a decidable, scheme-level check. No two peers
    // share a scheme (the second is unreachable); a `None` catch-all (`Visitor`)
    // accepts everyone, so it must come last. The compiler does not reason about
    // predicate-level disjointness — that is what keeps this decidable (Q4).
    if by.is_sum() {
        let mut seen: Vec<actors::Scheme> = Vec::new();
        let mut seen_catch_all = false;
        for (actor_ref, contract) in &members {
            if seen_catch_all {
                errors.push(
                    CompileError::new(
                        "bynk.actor.unreachable_sum_arm",
                        actor_ref.span,
                        format!(
                            "actor `{}` is unreachable — an earlier `None` peer accepts every caller",
                            actor_ref.name
                        ),
                    )
                    .with_note("a catch-all (`None`, e.g. `Visitor`) peer must come last"),
                );
                continue;
            }
            if contract.scheme == actors::Scheme::None {
                seen_catch_all = true;
            } else if seen.contains(&contract.scheme) {
                errors.push(
                    CompileError::new(
                        "bynk.actor.duplicate_sum_scheme",
                        actor_ref.span,
                        format!(
                            "actor `{}` repeats the `{}` scheme of an earlier peer",
                            actor_ref.name,
                            contract.scheme.as_str()
                        ),
                    )
                    .with_note(
                        "peers in a sum are distinguished by scheme — two same-scheme \
                         peers can't both be reached",
                    ),
                );
            } else {
                seen.push(contract.scheme);
            }
        }
    }
}

fn check_actor_contracts(
    table: &UnitTable,
    resolved: &ResolvedCommons,
    refs: &mut RefSink,
    errors: &mut Vec<CompileError>,
) {
    use bynk_check::actors::{self, Scheme};

    // Pass 1 — actor declaration well-formedness.
    for actor in table.actors.values() {
        refs.set_owner(&actor.name.name);
        // v0.53: a refinement actor (`actor Admin = User where <pred>`) carries
        // an authorisation invariant. Its base MUST be a declared `Bearer` actor
        // (only Bearer carries claims to authorise against), and its `where`
        // predicate MUST be in the closed claim-predicate set.
        if let Some(r) = &actor.refinement {
            let base = table.actors.get(&r.base.name);
            let base_is_bearer = base.is_some_and(|b| {
                b.refinement.is_none()
                    && b.auth.as_ref().and_then(|a| Scheme::from_name(&a.name))
                        == Some(Scheme::Bearer)
            });
            if base_is_bearer {
                refs.record(r.base.span, SymbolKind::Actor, &r.base.name);
            } else {
                errors.push(
                    CompileError::new(
                        "bynk.actor.refinement_base_unsupported",
                        r.base.span,
                        format!(
                            "the base actor `{}` of refinement `{}` must be a declared `Bearer` actor",
                            r.base.name, actor.name.name,
                        ),
                    )
                    .with_note(
                        "authorisation invariants test JWT claims, which only a `Bearer` actor \
                         carries — refine a `Bearer` actor, not `None`/`Internal`/`Signature`",
                    ),
                );
            }
            if let Err(span) = actors::parse_claim_predicate(&r.predicate) {
                errors.push(
                    CompileError::new(
                        "bynk.actor.refinement_predicate_unsupported",
                        span,
                        "a refinement predicate must be `hasClaim(\"…\")` or `claimEquals(\"…\", \"…\")`, composed with `&&`, `||`, `!`",
                    )
                    .with_note(
                        "claims are untyped JSON, so the predicate vocabulary is a closed set this \
                         slice; a general typed-claims surface is a later slice",
                    ),
                );
            }
            continue;
        }
        let Some(auth) = &actor.auth else {
            continue;
        };
        match Scheme::from_name(&auth.name) {
            None => errors.push(
                CompileError::new(
                    "bynk.actor.unknown_scheme",
                    auth.span,
                    format!("unknown authentication scheme `{}`", auth.name),
                )
                .with_note(
                    "the authentication schemes are `None`, `Internal`, `Bearer`, and `Signature`",
                ),
            ),
            // v0.47: a Bearer actor must name its signing secret and yield a
            // string-constructible identity (minted from the JWT `sub` claim).
            Some(Scheme::Bearer) => {
                if actor.scheme_arg("secret").is_none() {
                    errors.push(
                        CompileError::new(
                            "bynk.actor.bearer_missing_secret",
                            auth.span,
                            "a `Bearer` actor must name its signing secret",
                        )
                        .with_note(
                            "write `auth = Bearer(secret = \"<ENV_NAME>\")` — the env var the \
                             `Secrets` capability resolves to the JWT signing key",
                        ),
                    );
                }
                match &actor.identity {
                    None => errors.push(
                        CompileError::new(
                            "bynk.actor.bearer_identity_not_string_constructible",
                            auth.span,
                            "a `Bearer` actor must declare a string-constructible `identity`",
                        )
                        .with_note(
                            "the verified identity is minted from the token's `sub` claim — \
                             declare `identity = T` where `T` is a refined or opaque `String`",
                        ),
                    ),
                    Some(id) if !is_string_constructible(id, &resolved.types) => errors.push(
                        CompileError::new(
                            "bynk.actor.bearer_identity_not_string_constructible",
                            id.span(),
                            "a `Bearer` actor's identity must be string-constructible",
                        )
                        .with_note(
                            "the identity is minted from the token's `sub` claim (a string) — \
                             use a refined or opaque `String` type",
                        ),
                    ),
                    Some(_) => {}
                }
            }
            // v0.51: a Signature actor must name its secret and signature header;
            // a `tolerance` requires a `timestamp`; identity is `()` (a declared
            // identity is not yet supported).
            Some(Scheme::Signature) => {
                if actor.scheme_arg("secret").is_none() {
                    errors.push(
                        CompileError::new(
                            "bynk.actor.signature_missing_secret",
                            auth.span,
                            "a `Signature` actor must name its signing secret",
                        )
                        .with_note(
                            "write `auth = Signature(secret = \"<ENV_NAME>\", header = \"<Header>\")`",
                        ),
                    );
                }
                if actor.scheme_arg("header").is_none() {
                    errors.push(
                        CompileError::new(
                            "bynk.actor.signature_missing_header",
                            auth.span,
                            "a `Signature` actor must name the signature header",
                        )
                        .with_note(
                            "write `header = \"<Header-Name>\"` — the request header carrying the HMAC",
                        ),
                    );
                }
                if let Some(tol) = actor.scheme_arg("tolerance")
                    && actor.scheme_arg("timestamp").is_none()
                {
                    errors.push(
                        CompileError::new(
                            "bynk.actor.signature_tolerance_without_timestamp",
                            tol.span,
                            "`tolerance` requires a `timestamp` header to check against",
                        )
                        .with_note("add `timestamp = \"<Header>\"`, or drop `tolerance`"),
                    );
                }
                if let Some(id) = &actor.identity {
                    errors.push(
                        CompileError::new(
                            "bynk.actor.signature_identity_unsupported",
                            id.span(),
                            "a `Signature` actor does not yet support a declared `identity`",
                        )
                        .with_note(
                            "a signature attests authenticity, not a principal — the event is the \
                             body param; use `by Webhook ()`",
                        ),
                    );
                }
            }
            // v0.151: an `Oidc` actor names its provider's public trust
            // parameters — `issuer` (checked against `iss`), `audience` (checked
            // against `aud`), and the `jwks` endpoint URL — and yields a
            // string-constructible identity minted from the verified `sub`
            // claim. It names **no secret**: the trust root is the provider's
            // published public key set, not a shared signing key.
            Some(Scheme::Oidc) => {
                if actor.scheme_arg("issuer").is_none() {
                    errors.push(
                        CompileError::new(
                            "bynk.actor.oidc_missing_issuer",
                            auth.span,
                            "an `Oidc` actor must name its `issuer`",
                        )
                        .with_note(
                            "write `auth = Oidc(issuer = \"https://issuer.example\", audience = \"<aud>\", jwks = \"<jwks-url>\")` — \
                             the `iss` the verified token must carry",
                        ),
                    );
                }
                if actor.scheme_arg("audience").is_none() {
                    errors.push(
                        CompileError::new(
                            "bynk.actor.oidc_missing_audience",
                            auth.span,
                            "an `Oidc` actor must name its `audience`",
                        )
                        .with_note(
                            "add `audience = \"<aud>\"` — the `aud` claim the token must be issued for (this API)",
                        ),
                    );
                }
                if actor.scheme_arg("jwks").is_none() {
                    errors.push(
                        CompileError::new(
                            "bynk.actor.oidc_missing_jwks",
                            auth.span,
                            "an `Oidc` actor must name its `jwks` endpoint",
                        )
                        .with_note(
                            "add `jwks = \"https://issuer.example/.well-known/jwks.json\"` — the public key set the verifier fetches",
                        ),
                    );
                }
                match &actor.identity {
                    None => errors.push(
                        CompileError::new(
                            "bynk.actor.oidc_identity_not_string_constructible",
                            auth.span,
                            "an `Oidc` actor must declare a string-constructible `identity`",
                        )
                        .with_note(
                            "the verified identity is minted from the token's `sub` claim — \
                             declare `identity = T` where `T` is a refined or opaque `String`",
                        ),
                    ),
                    Some(id) if !is_string_constructible(id, &resolved.types) => errors.push(
                        CompileError::new(
                            "bynk.actor.oidc_identity_not_string_constructible",
                            id.span(),
                            "an `Oidc` actor's identity must be string-constructible",
                        )
                        .with_note(
                            "the identity is minted from the token's `sub` claim (a string) — \
                             use a refined or opaque `String` type",
                        ),
                    ),
                    Some(_) => {}
                }
            }
            Some(_) => {}
        }
        // A declared identity must be a context-ownable (sealed) type — a type
        // this context declares, so it can only be minted inside the context.
        // (Signature handles its own identity rule above.)
        if Scheme::from_name(actor.auth.as_ref().map(|a| a.name.as_str()).unwrap_or(""))
            != Some(Scheme::Signature)
            && let Some(id) = &actor.identity
        {
            let ownable =
                matches!(id, TypeRef::Named(n) if resolved.local_type_names.contains(&n.name));
            if !ownable {
                errors.push(
                    CompileError::new(
                        "bynk.actor.identity_not_sealed",
                        id.span(),
                        "an actor identity must be a context-ownable value type",
                    )
                    .with_note(
                        "declare the identity as a type in this context so it is sealed — \
                         minted only inside the context and unforgeable downstream",
                    ),
                );
            }
        }
    }

    // Pass 2 — handler `by`-clause contracts.
    for service in table.services.values() {
        refs.set_owner(&service.name.name);
        for handler in &service.handlers {
            match &handler.by_clause {
                Some(by) => {
                    check_by_clause_contracts(
                        by,
                        Some(&handler.params),
                        &service.protocol,
                        table,
                        refs,
                        errors,
                    );
                }
                None => {
                    // No `by`: edge protocols (HTTP, WebSocket) have no safe
                    // default actor; the internal protocols inherit one.
                    if actors::default_actor(&service.protocol).is_none() {
                        // v0.103 (D-A): a WebSocket upgrade authenticates at the
                        // edge before the connection is accepted — `on open` must
                        // name its actor, no anonymous upgrade.
                        let (msg, note) = match &service.protocol {
                            ServiceProtocol::WebSocket { .. } => (
                                "a WebSocket `on open` handler must declare its actor with a `by` clause",
                                "the upgrade authenticates at the edge before accepting the connection — name the actor (`by user: Participant`), there is no anonymous upgrade",
                            ),
                            _ => (
                                "an HTTP handler must declare its actor with a `by` clause",
                                "HTTP has no safe default actor — a public route writes `by v: Visitor`; an authenticated route names its actor",
                            ),
                        };
                        errors.push(
                            CompileError::new("bynk.actor.missing_by_on_http", handler.span, msg)
                                .with_note(note),
                        );
                    }
                }
            }
        }
        // v0.155: a service-level `by` default is validated *indirectly* — the
        // normalization pass injects it into the handlers that omit their own
        // clause, and the loop above checks those copies. So when the default is
        // inherited by **no** handler (every handler overrides it, or the service
        // has no handlers), it is injected into nothing and would go unchecked —
        // a typo'd/unknown default actor could pass silently, then surface later
        // at the header the moment an override is removed. Validate it directly
        // here in exactly that case, against the header span, so the diagnostic
        // is neither missed nor duplicated with the inherited-handler path.
        if let Some(default_by) = &service.default_by {
            let inherited = service.handlers.iter().any(|h| {
                h.by_clause
                    .as_ref()
                    .is_some_and(|b| b.span == default_by.span)
            });
            if !inherited {
                check_by_clause_contracts(default_by, None, &service.protocol, table, refs, errors);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn check_service_decls(
    typed: &mut checker::TypedCommons,
    table: &UnitTable,
    cross_context: &resolver::CrossContextInfo,
    resolved: &ResolvedCommons,
    capability_info_map: &HashMap<String, CapabilityInfo>,
    refs: &mut RefSink,
    hints: &mut HintSink,
    locals: &mut LocalsSink,
    requirements: &mut RequirementSink,
    errors: &mut Vec<CompileError>,
) {
    // v0.44: a service is one protocol adapter — every handler's form must
    // match the service's `from <protocol>` header.
    check_service_protocols(table, errors);

    // v0.45: actor-contract well-formedness and the handler `by`-clause checks.
    check_actor_contracts(table, resolved, refs, errors);

    // v0.9: validate HTTP handler shape and check for duplicate routes
    // across all services in this context.
    let mut route_first_span: HashMap<(HttpMethod, String), Span> = HashMap::new();
    for service in table.services.values() {
        for handler in &service.handlers {
            let HandlerKind::Http { method, path } = &handler.kind else {
                continue;
            };
            validate_http_handler(handler, *method, path, &typed.types, errors);
            let key = (*method, path.clone());
            if let Some(prev) = route_first_span.get(&key).copied() {
                errors.push(
                    CompileError::new(
                        "bynk.http.duplicate_route",
                        handler.span,
                        format!(
                            "duplicate HTTP route: another handler already declares `{} {}`",
                            method.as_str(),
                            path,
                        ),
                    )
                    .with_label(prev, "previously declared here"),
                );
            } else {
                route_first_span.insert(key, handler.span);
            }
        }
    }

    // v0.140 (ADR 0163): validate handler-position annotations (`@cache`) across
    // every handler — services and agents — so a misplaced annotation is caught
    // wherever it is written, not only on well-formed HTTP routes.
    for service in table.services.values() {
        for handler in &service.handlers {
            validate_handler_annotations(handler, errors);
        }
    }
    for agent in table.agents.values() {
        for handler in &agent.handlers {
            validate_handler_annotations(handler, errors);
        }
    }

    // v0.131 (ADR 0159): validate each service's `cors { }` policy.
    for service in table.services.values() {
        if let Some(policy) = &service.cors {
            validate_cors_policy(service, policy, errors);
        }
    }

    // v0.141 (ADR 0164): validate each service's `security { }` policy. (Absence
    // is legal and still stamps the safe defaults — only a *declared* block is
    // validated here.)
    for service in table.services.values() {
        if let Some(policy) = &service.security {
            validate_security_policy(service, policy, errors);
        }
    }

    // v0.142 (ADR 0165): validate each service's `limits { }` policy. (Absence is
    // legal — a service with no cap is unchanged; only a *declared* block is
    // validated here.)
    for service in table.services.values() {
        if let Some(policy) = &service.limits {
            validate_limits_policy(service, policy, errors);
        }
    }

    // v0.10a: validate `on cron` handler shape and check for duplicate
    // schedules across all services in this context (the generated
    // `scheduled` dispatcher routes on `event.cron`, so duplicates are
    // ambiguous).
    let mut schedule_first_span: HashMap<String, Span> = HashMap::new();
    for service in table.services.values() {
        for handler in &service.handlers {
            let HandlerKind::Cron { expr } = &handler.kind else {
                continue;
            };
            validate_cron_handler(handler, expr, errors);
            if let Some(prev) = schedule_first_span.get(expr).copied() {
                errors.push(
                    CompileError::new(
                        "bynk.cron.duplicate_schedule",
                        handler.span,
                        format!(
                            "duplicate cron schedule: another handler already declares `{expr}`",
                        ),
                    )
                    .with_label(prev, "previously declared here"),
                );
            } else {
                schedule_first_span.insert(expr.clone(), handler.span);
            }
        }
    }

    // v0.10b: validate `on queue` handler shape and check for duplicate
    // consumers across all services in this context (the generated `queue`
    // dispatcher routes on `batch.queue`, so two consumers of the same queue
    // are ambiguous).
    let mut consumer_first_span: HashMap<String, Span> = HashMap::new();
    for service in table.services.values() {
        let ServiceProtocol::Queue { name } = &service.protocol else {
            continue;
        };
        for handler in &service.handlers {
            if !matches!(handler.kind, HandlerKind::Message) {
                continue;
            }
            validate_queue_handler(handler, name, errors);
            if let Some(prev) = consumer_first_span.get(name).copied() {
                errors.push(
                    CompileError::new(
                        "bynk.queue.duplicate_consumer",
                        handler.span,
                        format!(
                            "duplicate queue consumer: another handler already consumes `{name}`",
                        ),
                    )
                    .with_label(prev, "previously declared here"),
                );
            } else {
                consumer_first_span.insert(name.clone(), handler.span);
            }
        }
    }

    // Check service handlers.
    for service in table.services.values() {
        refs.set_owner(&service.name.name);
        for handler in &service.handlers {
            // The given clause must reference only declared (local) or
            // exported (cross-context) capabilities.
            let mut handler_caps: HashMap<String, CapabilityInfo> = HashMap::new();
            for cap_ref in &handler.given {
                if let Some(info) =
                    resolve_given_cap_ref(cap_ref, capability_info_map, cross_context, errors, refs)
                {
                    handler_caps.insert(cap_ref.key().to_string(), info);
                }
            }
            // The handler return type must be Effect[T].
            if !matches!(handler.return_type, TypeRef::Effect(_, _)) {
                errors.push(CompileError::new(
                    "bynk.service.return_not_effect",
                    handler.return_type.span(),
                    format!(
                        "service handler must return `Effect[T]`, but got `{}`",
                        ts_type_ref_display(&handler.return_type)
                    ),
                ));
            }
            // v0.45: the `by`-bound actor identity, in scope for the body.
            let actor_binding = handler_actor_binding(handler, &service.protocol, table, resolved);
            // v0.103 (real-time track slice 3): an `on open` handler receives a
            // fresh owned `Connection[out]` named `connection`. Inject it as a
            // synthetic first parameter so the body type-checks against it and
            // the linearity pass seeds it as an owned held binding the handler
            // must dispose (transfer to an agent).
            // v0.103/v0.106: a `from websocket` lifecycle handler receives the
            // `connection` as a synthetic first param — the fresh owned socket for
            // `on open` (which must be disposed/transferred), or the **borrowed**
            // firing socket for `on message`/`on close` (used non-consumingly, never
            // disposed by the handler). The body type-checks against it either way;
            // the linearity pass treats the borrowed cases via `borrowed_held`.
            let is_ws_lifecycle = matches!(
                (&handler.kind, &service.protocol),
                (
                    HandlerKind::Open | HandlerKind::Message | HandlerKind::Close,
                    ServiceProtocol::WebSocket { .. }
                )
            );
            let params_for_check: Vec<Param> = match (&handler.kind, &service.protocol) {
                (
                    HandlerKind::Open | HandlerKind::Message | HandlerKind::Close,
                    ServiceProtocol::WebSocket { out_type, .. },
                ) => {
                    let mut ps = vec![open_connection_param(out_type, handler.span)];
                    ps.extend(handler.params.iter().cloned());
                    ps
                }
                _ => handler.params.clone(),
            };
            // The firing `connection` of `on message`/`on close` is borrowed, not
            // owned — no disposal obligation (contrast `on open`, owned).
            let borrowed_held: std::collections::HashSet<String> = if is_ws_lifecycle
                && matches!(handler.kind, HandlerKind::Message | HandlerKind::Close)
            {
                std::iter::once("connection".to_string()).collect()
            } else {
                std::collections::HashSet::new()
            };
            checker::check_handler_body(
                resolved,
                checker::HandlerBodyCheck {
                    capabilities: handler_caps,
                    declared_capabilities: capability_info_map.clone(),
                    given_anchor: Some(handler.return_type.span()),
                    report_unused: true,
                    actor_binding,
                    borrowed_held,
                    ..checker::HandlerBodyCheck::new(
                        &handler.body,
                        &handler.return_type,
                        &params_for_check,
                        &handler.given,
                    )
                },
                checker::CheckSinks {
                    expr_types: &mut typed.expr_types,
                    errors,
                    refs,
                    hints,
                    locals,
                    requirements,
                },
            );
        }
        // v0.155: like the `by` default (see check_actor_contracts), a service-
        // level `given` default is validated only through the handlers that
        // inherit it — the normalization pass injects it into handlers that
        // declare no `given` of their own. When it is inherited by no handler
        // (every handler declares its own `given`), resolve the default's
        // capabilities directly here so an unknown/typo'd default capability is
        // still reported, at the header. (A service always has ≥1 handler, so the
        // zero-handler case cannot arise; only full shadowing.)
        if let Some(first) = service.default_given.first() {
            let inherited = service
                .handlers
                .iter()
                .any(|h| h.given.first().is_some_and(|g| g.span == first.span));
            if !inherited {
                for cap_ref in &service.default_given {
                    let _ = resolve_given_cap_ref(
                        cap_ref,
                        capability_info_map,
                        cross_context,
                        errors,
                        refs,
                    );
                }
            }
        }
    }
}

/// v0.103: the synthetic `connection: Connection[out]` parameter an `on open`
/// handler receives — a fresh, owned held binding the framework supplies and the
/// handler must dispose (§2.9.4).
fn open_connection_param(out_type: &TypeRef, span: Span) -> Param {
    Param {
        name: Ident {
            name: "connection".to_string(),
            span,
        },
        type_ref: TypeRef::Connection(Box::new(out_type.clone()), span),
        span,
    }
}

/// v0.45: the actor binding a service handler exposes to its body, if it has a
/// `by <binder>: <Actor>` clause. Returns `(binder, identity_ty)`. Default-actor
/// handlers (no `by`) carry no named binding. The identity type is the actor's
/// declared `identity = T` (a context-ownable type), or the scheme default:
/// `()` for trivial actors, the calling-context id (`String`) for the prelude
/// `Caller` (Q7).
fn handler_actor_binding(
    handler: &Handler,
    _protocol: &ServiceProtocol,
    table: &UnitTable,
    resolved: &ResolvedCommons,
) -> Option<(String, checker::Ty)> {
    let by = handler.by_clause.as_ref()?;
    // No binder (binder-less `by <Actor>`) ⇒ no identity binding in scope.
    let binder = by.binder.as_ref()?;
    // A binder that collides with a parameter is diagnosed
    // (`bynk.actor.binder_shadows_param`); suppress the binding so the body
    // scope keeps the real parameter rather than the clobbering actor binding.
    if handler.params.iter().any(|p| p.name.name == binder.name) {
        return None;
    }
    // v0.52: a sum (`by who: A | B`) binds an `ActorSum` the body matches; a
    // single actor binds an `Actor` exposing `.identity`.
    let binder_ty = if by.is_sum() {
        checker::Ty::ActorSum(
            by.actors
                .iter()
                .map(|a| (a.name.clone(), actor_identity_ty(&a.name, table, resolved)))
                .collect(),
        )
    } else {
        checker::Ty::Actor(Box::new(actor_identity_ty(
            &by.primary().name,
            table,
            resolved,
        )))
    };
    Some((binder.name.clone(), binder_ty))
}

/// The identity `Ty` a named actor yields (a local declaration or a prelude
/// actor).
fn actor_identity_ty(
    actor_name: &str,
    table: &UnitTable,
    resolved: &ResolvedCommons,
) -> checker::Ty {
    actor_identity_ty_guarded(actor_name, table, resolved, &mut Vec::new())
}

/// Inner worker carrying a `seen` chain so a malformed **refinement cycle**
/// (`actor A = A`, or `A = B` / `B = A`) terminates with the unit identity
/// instead of overflowing the stack. A valid refinement's base is a direct
/// `Bearer` actor (the checker rejects refinement chains/cycles with
/// `refinement_base_unsupported`), so this guard only ever fires on input that
/// is already a compile error — it keeps the checker from crashing before that
/// diagnostic is reported.
fn actor_identity_ty_guarded<'a>(
    actor_name: &'a str,
    table: &'a UnitTable,
    resolved: &ResolvedCommons,
    seen: &mut Vec<&'a str>,
) -> checker::Ty {
    use bynk_check::actors::{Identity, prelude_actor};
    if let Some(local) = table.actors.get(actor_name) {
        // v0.53: a refinement actor (`actor Admin = User where …`) yields its
        // base's identity — refinement elimination, an `Admin` is-a `User`.
        if let Some(r) = &local.refinement {
            if seen.contains(&actor_name) {
                return checker::Ty::Unit;
            }
            seen.push(actor_name);
            // Resolve against the declaration's own key so the cycle guard sees
            // the same name on a self-reference.
            if let Some((key, _)) = table.actors.get_key_value(&r.base.name) {
                return actor_identity_ty_guarded(key.as_str(), table, resolved, seen);
            }
            return checker::Ty::Unit;
        }
        return match &local.identity {
            Some(id) => checker::resolve_type_ref(id, &resolved.types).unwrap_or(checker::Ty::Unit),
            None => checker::Ty::Unit,
        };
    }
    match prelude_actor(actor_name).map(|c| c.identity) {
        Some(Identity::CallerId) => checker::Ty::Base(bynk_syntax::ast::BaseType::String),
        _ => checker::Ty::Unit,
    }
}

/// The closed storage-kind catalogue (design notes §10). `Cell` and `Map` are
/// functional; the rest (`Set`/`Log`/`Queue`/`Cache`) parse and validate as known
/// kinds but are gated (`bynk.store.kind_unsupported`).
const STORAGE_KINDS: &[&str] = &["Cell", "Map", "Set", "Log", "Queue", "Cache"];

/// The closed storage-annotation registry (ADR 0111 D2/D3): each `@name` with the
/// storage kind(s) it attaches to and the slice that makes it functional. v0.85
/// (slice 3a) lands the grammar + registry; every annotation is gated
/// (`bynk.store.annotation_unsupported`) until its slice — so `functional` is
/// `false` for all of them here, flipped per-name as later slices land.
struct AnnotationSpec {
    name: &'static str,
    kinds: &'static [&'static str],
    slice: &'static str,
    functional: bool,
}

const ANNOTATIONS: &[AnnotationSpec] = &[
    AnnotationSpec {
        name: "ttl",
        kinds: &["Cache"],
        slice: "the Cache slice",
        functional: true,
    },
    AnnotationSpec {
        name: "retain",
        kinds: &["Log"],
        slice: "the Log slice",
        functional: true,
    },
    AnnotationSpec {
        name: "indexed",
        kinds: &["Map"],
        slice: "the query-algebra track",
        functional: true,
    },
    AnnotationSpec {
        name: "bounded",
        kinds: &["Queue", "Log"],
        slice: "the Queue/Log slices",
        functional: false,
    },
];

/// Validate a `store` field's annotations against the closed registry (ADR 0111):
/// an unknown name is `bynk.store.unknown_annotation`; a known name on the wrong
/// kind is `bynk.store.annotation_kind_mismatch`; a known name on the right kind
/// whose slice has not landed is `bynk.store.annotation_unsupported`. `head` is
/// the (already known-valid) storage kind of the field.
fn validate_store_annotations(
    f: &StoreField,
    head: &str,
    types: &HashMap<String, TypeDecl>,
    errors: &mut Vec<CompileError>,
) {
    for ann in &f.annotations {
        let name = ann.name.name.as_str();
        let Some(spec) = ANNOTATIONS.iter().find(|s| s.name == name) else {
            errors.push(
                CompileError::new(
                    "bynk.store.unknown_annotation",
                    ann.name.span,
                    format!(
                        "unknown storage annotation `@{name}` — expected one of {}",
                        ANNOTATIONS
                            .iter()
                            .map(|s| format!("@{}", s.name))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                )
                .with_note("storage annotations are a closed set (ADR 0111)"),
            );
            continue;
        };
        if !spec.kinds.contains(&head) {
            errors.push(CompileError::new(
                "bynk.store.annotation_kind_mismatch",
                ann.span,
                format!(
                    "`@{name}` applies to {}, not `{head}`",
                    spec.kinds
                        .iter()
                        .map(|k| format!("`{k}`"))
                        .collect::<Vec<_>>()
                        .join("/")
                ),
            ));
            continue;
        }
        if !spec.functional {
            errors.push(
                CompileError::new(
                    "bynk.store.annotation_unsupported",
                    ann.span,
                    format!(
                        "`@{name}` is not yet supported — it lands with {}",
                        spec.slice
                    ),
                )
                .with_note(
                    "the annotation grammar is in place; its meaning arrives with its slice",
                ),
            );
            continue;
        }
        // v0.93 (ADR 0118): `@indexed(by: k, …)` — each `by:` names a
        // **value-keyable field of the map's value type** to maintain a secondary
        // index on. Validate the keys here, now the kind/value type are known.
        if name == "indexed" {
            validate_indexed_keys(f, types, ann, errors);
        }
    }
}

/// v0.93 (ADR 0118): each `@indexed(by: k)` key must label a `by:` argument that
/// names a **value-keyable field** of the map's value type (a `Record`). A
/// non-`by:` argument, a key that is not a field, or a non-keyable field type is
/// a diagnostic.
fn validate_indexed_keys(
    f: &StoreField,
    types: &HashMap<String, TypeDecl>,
    ann: &Annotation,
    errors: &mut Vec<CompileError>,
) {
    // The map's value type is the second kind argument (`Map[K, V]`).
    let value_fields: Option<&[RecordField]> = f
        .kind
        .args
        .get(1)
        .and_then(|v| match v {
            TypeRef::Named(id) => types.get(&id.name),
            _ => None,
        })
        .and_then(|decl| match &decl.body {
            TypeBody::Record(r) => Some(r.fields.as_slice()),
            _ => None,
        });
    for arg in &ann.args {
        // Only `by:` labels are admitted on `@indexed`.
        let Some(label) = &arg.label else {
            errors.push(CompileError::new(
                "bynk.index.bad_argument",
                arg.span,
                "`@indexed` arguments are `by: <field>` labels naming a field to index on",
            ));
            continue;
        };
        if label.name != "by" {
            errors.push(CompileError::new(
                "bynk.index.bad_argument",
                arg.span,
                format!("`@indexed` takes `by:` arguments, not `{}:`", label.name),
            ));
            continue;
        }
        let ExprKind::Ident(key) = &arg.value.kind else {
            errors.push(CompileError::new(
                "bynk.index.bad_argument",
                arg.value.span,
                "`@indexed(by: …)` names a field of the map's value type",
            ));
            continue;
        };
        // The value type must be a record whose field `key` exists and is keyable.
        match value_fields.and_then(|fs| fs.iter().find(|rf| rf.name.name == key.name)) {
            None => {
                errors.push(CompileError::new(
                    "bynk.index.unknown_key",
                    arg.value.span,
                    format!(
                        "`@indexed(by: {0})` — the map's value type has no field `{0}`",
                        key.name
                    ),
                ));
            }
            Some(field) if !type_ref_is_keyable(&field.type_ref, types) => {
                errors.push(
                    CompileError::new(
                        "bynk.index.unkeyable_key",
                        arg.value.span,
                        format!(
                            "`@indexed(by: {0})` — field `{0}` is not value-keyable; an index key must be `Int`, `String`, or a refined/opaque type over them",
                            key.name
                        ),
                    ),
                );
            }
            Some(_) => {}
        }
    }
}

/// Whether a `TypeRef` is value-keyable (the Map-key / index-key rule, ADR 0110
/// D5): `Int`/`String`, including a refined/opaque named type over them.
fn type_ref_is_keyable(t: &TypeRef, types: &HashMap<String, TypeDecl>) -> bool {
    match t {
        TypeRef::Base(BaseType::Int | BaseType::String, _) => true,
        TypeRef::Named(id) => matches!(
            types.get(&id.name).map(|d| &d.body),
            Some(TypeBody::Refined { base, .. } | TypeBody::Opaque { base, .. })
                if matches!(base, BaseType::Int | BaseType::String)
        ),
        _ => false,
    }
}

/// v0.93 (ADR 0118 D4): index-hygiene **warnings** (non-failing, via ADR 0117).
/// Cross-references the agent's `@indexed(by: …)` declarations against the
/// equality `filter`s in its handlers:
///   - `bynk.index.missing` — an equality `filter` on a non-indexed keyable field
///     (the lookup scans; an index would route it);
///   - `bynk.index.unused` — a declared index no equality `filter` routes through
///     (it costs maintenance on every write).
///
/// These are perf hints, never compile gates (§11). The selectivity/ambiguity
/// tie-break (D5) and compound-predicate routing are a named follow-on, so a
/// single-equality predicate (the only shape routed today) is never ambiguous.
fn validate_index_hygiene(
    agent: &AgentDecl,
    types: &HashMap<String, TypeDecl>,
    errors: &mut Vec<CompileError>,
) {
    let mut store_maps: HashSet<String> = HashSet::new();
    // map → declared (field, span-of-the-`by:`-argument)
    let mut declared: HashMap<String, Vec<(String, Span)>> = HashMap::new();
    // map → the value type's record fields (for the keyability check)
    let mut value_fields: HashMap<String, Vec<RecordField>> = HashMap::new();
    for f in &agent.store_fields {
        if f.kind.head.name != "Map" || f.kind.args.len() != 2 {
            continue;
        }
        store_maps.insert(f.name.name.clone());
        if let Some(TypeBody::Record(r)) = f
            .kind
            .args
            .get(1)
            .and_then(|v| match v {
                TypeRef::Named(id) => types.get(&id.name),
                _ => None,
            })
            .map(|d| &d.body)
        {
            value_fields.insert(f.name.name.clone(), r.fields.clone());
        }
        for an in f.annotations.iter().filter(|a| a.name.name == "indexed") {
            for arg in &an.args {
                if arg.label.as_ref().map(|l| l.name.as_str()) == Some("by")
                    && let ExprKind::Ident(k) = &arg.value.kind
                {
                    declared
                        .entry(f.name.name.clone())
                        .or_default()
                        .push((k.name.clone(), arg.value.span));
                }
            }
        }
    }
    if store_maps.is_empty() {
        return;
    }
    // Walk every handler body for equality filters in the routable position
    // (`<map>.filter((r) => r.f == …)`), recording the (map, field) pairs hit and
    // warning about a missing index the first time a field is filtered on.
    let mut used: HashSet<(String, String)> = HashSet::new();
    let mut missing_seen: HashSet<(String, String)> = HashSet::new();
    for h in &agent.handlers {
        walk_block_for_index_filters(&h.body, &store_maps, &mut |map, field, span| {
            used.insert((map.to_string(), field.to_string()));
            let is_declared = declared
                .get(map)
                .is_some_and(|v| v.iter().any(|(f, _)| f == field));
            if is_declared {
                return;
            }
            let keyable = value_fields.get(map).is_some_and(|fs| {
                fs.iter()
                    .any(|rf| rf.name.name == field && type_ref_is_keyable(&rf.type_ref, types))
            });
            if keyable && missing_seen.insert((map.to_string(), field.to_string())) {
                errors.push(
                    CompileError::new(
                        "bynk.index.missing",
                        span,
                        format!(
                            "a query filters `{map}` by equality on `{field}`, which is not indexed — add `@indexed(by: {field})` to route this lookup through an index instead of a scan"
                        ),
                    )
                    .with_note("a perf hint, not an error — the scan still compiles and runs"),
                );
            }
        });
    }
    // A declared index no equality filter routes through is dead maintenance.
    for (map, fields) in &declared {
        for (field, span) in fields {
            if !used.contains(&(map.clone(), field.clone())) {
                errors.push(
                    CompileError::new(
                        "bynk.index.unused",
                        *span,
                        format!(
                            "`@indexed(by: {field})` on `{map}` is never used — no query filters `{map}` by equality on `{field}`, yet the index is maintained on every write"
                        ),
                    )
                    .with_note("remove it, or add a query that filters by equality on this field"),
                );
            }
        }
    }
}

/// `<map>.filter((r) => r.<field> == …)` with `map` a store map → `(map, field)`.
/// The routable equality-filter shape (the only one [`route_indexed_filter`]
/// lowers); deeper-in-a-chain filters cannot route, so they are not hygiene-relevant.
fn routable_eq_filter<'a>(
    store_maps: &HashSet<String>,
    e: &'a Expr,
) -> Option<(&'a str, &'a str, Span)> {
    let ExprKind::MethodCall {
        receiver,
        method,
        args,
        ..
    } = &e.kind
    else {
        return None;
    };
    if method.name != "filter" {
        return None;
    }
    let ExprKind::Ident(map) = &receiver.kind else {
        return None;
    };
    if !store_maps.contains(&map.name) {
        return None;
    }
    let [arg] = args.as_slice() else {
        return None;
    };
    let ExprKind::Lambda(lam) = &arg.kind else {
        return None;
    };
    let [param] = lam.params.as_slice() else {
        return None;
    };
    let pname = param.name.name.as_str();
    let ExprKind::BinOp(BinOp::Eq, lhs, rhs) = &lam.body.kind else {
        return None;
    };
    let field_of = |x: &'a Expr| -> Option<&'a str> {
        if let ExprKind::FieldAccess { receiver, field } = &x.kind
            && let ExprKind::Ident(r) = &receiver.kind
            && r.name == pname
        {
            Some(field.name.as_str())
        } else {
            None
        }
    };
    let field = field_of(lhs).or_else(|| field_of(rhs))?;
    Some((map.name.as_str(), field, e.span))
}

/// Recurse a block, invoking `cb(map, field, span)` for each routable equality
/// filter found anywhere in it.
fn walk_block_for_index_filters(
    block: &Block,
    store_maps: &HashSet<String>,
    cb: &mut dyn FnMut(&str, &str, Span),
) {
    for stmt in &block.statements {
        let v = match stmt {
            Statement::Let(l) | Statement::EffectLet(l) => &l.value,
            Statement::Expect(a) => &a.value,
            Statement::Send(s) => &s.value,
            Statement::Do(d) => &d.value,
            Statement::Assign(a) => &a.value,
        };
        walk_expr_for_index_filters(v, store_maps, cb);
    }
    walk_expr_for_index_filters(&block.tail, store_maps, cb);
}

/// Recurse an expression, invoking `cb` for each routable equality filter.
fn walk_expr_for_index_filters(
    e: &Expr,
    store_maps: &HashSet<String>,
    cb: &mut dyn FnMut(&str, &str, Span),
) {
    if let Some((map, field, span)) = routable_eq_filter(store_maps, e) {
        cb(map, field, span);
    }
    match &e.kind {
        ExprKind::MethodCall { receiver, args, .. } => {
            walk_expr_for_index_filters(receiver, store_maps, cb);
            for a in args {
                walk_expr_for_index_filters(a, store_maps, cb);
            }
        }
        ExprKind::FieldAccess { receiver, .. } => {
            walk_expr_for_index_filters(receiver, store_maps, cb)
        }
        ExprKind::BinOp(_, l, r) => {
            walk_expr_for_index_filters(l, store_maps, cb);
            walk_expr_for_index_filters(r, store_maps, cb);
        }
        ExprKind::UnaryOp(_, x)
        | ExprKind::Paren(x)
        | ExprKind::Question(x)
        | ExprKind::Ok(x)
        | ExprKind::Err(x)
        | ExprKind::Some(x)
        | ExprKind::EffectPure(x)
        | ExprKind::Expect(x) => walk_expr_for_index_filters(x, store_maps, cb),
        ExprKind::Lambda(lam) => walk_expr_for_index_filters(&lam.body, store_maps, cb),
        ExprKind::Block(b) => walk_block_for_index_filters(b, store_maps, cb),
        ExprKind::If {
            cond,
            then_block,
            else_block,
        } => {
            walk_expr_for_index_filters(cond, store_maps, cb);
            walk_block_for_index_filters(then_block, store_maps, cb);
            walk_block_for_index_filters(else_block, store_maps, cb);
        }
        ExprKind::Match { discriminant, arms } => {
            walk_expr_for_index_filters(discriminant, store_maps, cb);
            for arm in arms {
                match &arm.body {
                    MatchBody::Expr(x) => walk_expr_for_index_filters(x, store_maps, cb),
                    MatchBody::Block(b) => walk_block_for_index_filters(b, store_maps, cb),
                }
            }
        }
        ExprKind::Is { value, .. } => walk_expr_for_index_filters(value, store_maps, cb),
        ExprKind::Call { args, .. }
        | ExprKind::ConstructorCall { args, .. }
        | ExprKind::Val { args, .. } => {
            for a in args {
                walk_expr_for_index_filters(a, store_maps, cb);
            }
        }
        ExprKind::RecordConstruction { fields, .. } => {
            for fi in fields {
                if let Some(v) = &fi.value {
                    walk_expr_for_index_filters(v, store_maps, cb);
                }
            }
        }
        ExprKind::RecordSpread {
            base, overrides, ..
        } => {
            walk_expr_for_index_filters(base, store_maps, cb);
            for fi in overrides {
                if let Some(v) = &fi.value {
                    walk_expr_for_index_filters(v, store_maps, cb);
                }
            }
        }
        ExprKind::ListLit(elems) => {
            for el in elems {
                walk_expr_for_index_filters(el, store_maps, cb);
            }
        }
        ExprKind::InterpStr(parts) => {
            for part in parts {
                if let InterpPart::Hole(h) = part {
                    walk_expr_for_index_filters(h, store_maps, cb);
                }
            }
        }
        _ => {}
    }
}

/// v0.81/v0.82 (storage track): validate an agent's `store`-field kinds and build
/// the per-kind scopes — `Cell` fields (name → element type; bare reads + `:=`)
/// and `Map` fields (name → (key, value) types; effectful entry ops, ADR 0110).
/// Unknown heads, bad arity, and not-yet-supported kinds are diagnosed.
#[allow(clippy::type_complexity)]
fn store_field_scopes(
    agent: &AgentDecl,
    types: &HashMap<String, TypeDecl>,
    no_vars: &HashSet<String>,
    refs: &mut RefSink,
    errors: &mut Vec<CompileError>,
) -> (
    HashMap<String, Ty>,
    HashMap<String, (Ty, Ty)>,
    HashMap<String, Ty>,
    HashMap<String, (Ty, Ty, i64)>,
    HashMap<String, Ty>,
) {
    let mut cells: HashMap<String, Ty> = HashMap::new();
    let mut maps: HashMap<String, (Ty, Ty)> = HashMap::new();
    let mut sets: HashMap<String, Ty> = HashMap::new();
    let mut caches: HashMap<String, (Ty, Ty, i64)> = HashMap::new();
    let mut logs: HashMap<String, Ty> = HashMap::new();
    let arity_err = |f: &StoreField, kind: &str, want: usize, errors: &mut Vec<CompileError>| {
        errors.push(CompileError::new(
            "bynk.store.kind_arity",
            f.kind.span,
            format!(
                "`{kind}` takes exactly {want} type argument(s), found {}",
                f.kind.args.len()
            ),
        ));
    };
    for f in &agent.store_fields {
        let head = f.kind.head.name.as_str();
        if !STORAGE_KINDS.contains(&head) {
            errors.push(
                CompileError::new(
                    "bynk.store.unknown_kind",
                    f.kind.head.span,
                    format!(
                        "unknown storage kind `{head}` — expected one of {}",
                        STORAGE_KINDS.join(", ")
                    ),
                )
                .with_note("a `store` field's type is a storage kind, not an ordinary type"),
            );
            continue;
        }
        // v0.85 (ADR 0111): validate any `@…` annotations now the kind is known.
        validate_store_annotations(f, head, types, errors);
        match head {
            "Cell" => {
                if f.kind.args.len() != 1 {
                    arity_err(f, "Cell", 1, errors);
                    continue;
                }
                let elem = &f.kind.args[0];
                checker::record_type_refs(elem, types, no_vars, refs);
                if let Some(ty) = checker::resolve_type_ref(elem, types) {
                    cells.insert(f.name.name.clone(), ty);
                }
            }
            "Map" => {
                if f.kind.args.len() != 2 {
                    arity_err(f, "Map", 2, errors);
                    continue;
                }
                checker::record_type_refs(&f.kind.args[0], types, no_vars, refs);
                checker::record_type_refs(&f.kind.args[1], types, no_vars, refs);
                if let (Some(k), Some(v)) = (
                    checker::resolve_type_ref(&f.kind.args[0], types),
                    checker::resolve_type_ref(&f.kind.args[1], types),
                ) {
                    maps.insert(f.name.name.clone(), (k, v));
                }
            }
            "Set" => {
                if f.kind.args.len() != 1 {
                    arity_err(f, "Set", 1, errors);
                    continue;
                }
                let elem = &f.kind.args[0];
                checker::record_type_refs(elem, types, no_vars, refs);
                if let Some(ty) = checker::resolve_type_ref(elem, types) {
                    sets.insert(f.name.name.clone(), ty);
                }
            }
            // v0.87 (ADR 0113): `Cache[K, V]` — a `Map` with per-entry TTL.
            "Cache" => {
                if f.kind.args.len() != 2 {
                    arity_err(f, "Cache", 2, errors);
                    continue;
                }
                checker::record_type_refs(&f.kind.args[0], types, no_vars, refs);
                checker::record_type_refs(&f.kind.args[1], types, no_vars, refs);
                // A `Cache` requires `@ttl(<Duration>)`; its millisecond value is
                // the entry lifetime. Absent → steer the author to a `Map`.
                let ttl = cache_ttl_millis(f, errors);
                if let (Some(k), Some(v), Some(ttl)) = (
                    checker::resolve_type_ref(&f.kind.args[0], types),
                    checker::resolve_type_ref(&f.kind.args[1], types),
                    ttl,
                ) {
                    caches.insert(f.name.name.clone(), (k, v, ttl));
                }
            }
            // v0.95 (ADR 0121): `Log[T]` — an append-only, time-indexed sequence.
            // The element type drives `append` and the lazy `Query[T]` read surface;
            // `@retain` (optional) is read by the emitter, not needed here.
            "Log" => {
                if f.kind.args.len() != 1 {
                    arity_err(f, "Log", 1, errors);
                    continue;
                }
                let elem = &f.kind.args[0];
                checker::record_type_refs(elem, types, no_vars, refs);
                if let Some(t) = checker::resolve_type_ref(elem, types) {
                    logs.insert(f.name.name.clone(), t);
                }
            }
            other => {
                errors.push(
                    CompileError::new(
                        "bynk.store.kind_unsupported",
                        f.kind.head.span,
                        format!(
                            "storage kind `{other}` is not yet supported — `Cell`, `Map`, \
                             `Set`, `Cache`, and `Log` are functional in this storage-track slice"
                        ),
                    )
                    .with_note("the remaining kind (`Queue`) follows in a later slice"),
                );
            }
        }
    }
    (cells, maps, sets, caches, logs)
}

/// v0.87 (ADR 0113 D2): a `Cache` field must carry `@ttl(<Duration literal>)`;
/// return its value in milliseconds. A missing `@ttl` is
/// `bynk.store.cache_ttl_required` (steering to a `Map`); a non-`Duration`
/// argument is caught by the annotation-argument checker, so here a malformed
/// `@ttl` simply yields `None`.
fn cache_ttl_millis(f: &StoreField, errors: &mut Vec<CompileError>) -> Option<i64> {
    let ttl = f.annotations.iter().find(|a| a.name.name == "ttl");
    let Some(ttl) = ttl else {
        errors.push(
            CompileError::new(
                "bynk.store.cache_ttl_required",
                f.kind.span,
                "a `Cache` field requires a `@ttl(<duration>)` annotation — its entry lifetime",
            )
            .with_note("a keyed store with no expiry is a `Map`, not a `Cache`"),
        );
        return None;
    };
    match ttl.args.first().map(|a| &a.value.kind) {
        Some(ExprKind::DurationLit { millis, .. }) => Some(*millis),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn check_agent_decls(
    typed: &mut checker::TypedCommons,
    table: &UnitTable,
    cross_context: &resolver::CrossContextInfo,
    capability_info_map: &HashMap<String, CapabilityInfo>,
    no_vars: &HashSet<String>,
    refs: &mut RefSink,
    hints: &mut HintSink,
    locals: &mut LocalsSink,
    requirements: &mut RequirementSink,
    errors: &mut Vec<CompileError>,
) {
    for agent in table.agents.values() {
        refs.set_owner(&agent.name.name);
        // v0.81 (storage track, emission slice — ADR 0109): `store` `Cell` fields
        // are checked (kind validity, bare reads, the `:=` write form, invariant
        // resolution) *and* emitted — the cells form the agent's state record,
        // written through a staged working copy committed atomically at handler
        // end. `store_cells` maps each `Cell` field to its element type, for the
        // bare-read scope and the `:=`/invariant checks below.
        #[allow(clippy::type_complexity)]
        let (store_cells, store_maps, store_sets, store_caches, store_logs): (
            HashMap<String, Ty>,
            HashMap<String, (Ty, Ty)>,
            HashMap<String, Ty>,
            HashMap<String, (Ty, Ty, i64)>,
            HashMap<String, Ty>,
        ) = if agent.store_fields.is_empty() {
            (
                HashMap::new(),
                HashMap::new(),
                HashMap::new(),
                HashMap::new(),
                HashMap::new(),
            )
        } else {
            store_field_scopes(agent, &typed.types, no_vars, refs, errors)
        };
        // v0.93 (ADR 0118 D4): index-hygiene warnings cross-reference `@indexed`
        // declarations against the equality filters in the handlers.
        validate_index_hygiene(agent, &typed.types, errors);
        // v0.25: the agent's key type and store field types reference types.
        checker::record_type_refs(&agent.key_type, &typed.types, no_vars, refs);
        for field in &agent.store_fields {
            for arg in &field.kind.args {
                checker::record_type_refs(arg, &typed.types, no_vars, refs);
            }
        }
        // The agent's `Cell` fields form its state record. Expose that record
        // under the name `<AgentName>State` in the type table so the body and
        // invariants can be checked against it.
        let agent_state_name = format!("{}State", agent.name.name);
        let state_record_fields: Vec<RecordField> = agent
            .store_fields
            .iter()
            .filter(|f| f.kind.head.name == "Cell" && f.kind.args.len() == 1)
            .map(|f| RecordField {
                name: f.name.clone(),
                type_ref: f.kind.args[0].clone(),
                refinement: None,
                init: f.init.clone(),
                span: f.span,
            })
            .collect();
        // Build a synthetic Record TypeDecl and stuff it into a *clone* of
        // the resolved types so handler bodies see it.
        let synthetic_state = TypeDecl {
            name: Ident {
                name: agent_state_name.clone(),
                span: agent.span,
            },
            type_params: Vec::new(),
            body: TypeBody::Record(RecordBody {
                fields: state_record_fields,
                span: agent.span,
            }),
            documentation: None,
            span: agent.span,
            trivia: Trivia::default(),
        };
        let mut types_for_handler = typed.types.clone();
        types_for_handler.insert(agent_state_name.clone(), synthetic_state.clone());
        let local_names_for_handler: std::collections::HashSet<String> =
            types_for_handler.keys().cloned().collect();
        let resolved_for_handler = ResolvedCommons {
            commons: typed.commons.clone(),
            types: types_for_handler,
            fns: typed.fns.clone(),
            methods: typed.methods.clone(),
            local_type_names: local_names_for_handler,
            cross_context: cross_context.clone(),
            agents: table.agents.clone(),
            imported_from: HashMap::new(),
        };
        // v0.81: the fresh-key rule for `store Cell[T]` fields — an
        // initialiser is checked against the element type `T` (which also types
        // the init expression so the emitter can qualify variant constructors),
        // and a field with neither an initialiser nor an implicit zero is rejected.
        for field in &agent.store_fields {
            if field.kind.head.name != "Cell" || field.kind.args.len() != 1 {
                continue; // non-Cell / malformed kinds are diagnosed elsewhere
            }
            let elem = &field.kind.args[0];
            if let Some(init) = &field.init {
                checker::check_state_initialiser(
                    init,
                    elem,
                    &resolved_for_handler,
                    &mut typed.expr_types,
                    errors,
                    refs,
                    hints,
                    locals,
                );
            } else if checker::zero_value_ts(elem, None, &typed.types).is_none() {
                errors.push(
                    CompileError::new(
                        "bynk.agents.non_zeroable_state_field",
                        field.span,
                        format!(
                            "agent `{}` store cell `{}` has no defined zero value, so a fresh \
                             key cannot be initialised",
                            agent.name.name, field.name.name
                        ),
                    )
                    .with_note(
                        "add an initialiser (`store name: Cell[T] = value`), or use \
                         `Cell[Option[…]]` (None means \"never set\")",
                    ),
                );
            }
        }
        let state_ty = Ty::Named {
            name: agent_state_name.clone(),
            kind: checker::NamedKind::Record,
            args: Vec::new(),
        };
        let key_ty = checker::resolve_type_ref(&agent.key_type, &typed.types).unwrap_or(Ty::Unit);
        let mut self_scope: HashMap<String, Ty> = HashMap::new();
        // `self` is a synthetic record carrying the agent's key field, so that
        // `self.<key>` resolves. The parser treats `self.x` as FieldAccess on
        // Ident("self"), so `self` is given a one-off synthetic record type.
        let agent_self_name = format!("__{}Self", agent.name.name);
        let self_decl = TypeDecl {
            name: Ident {
                name: agent_self_name.clone(),
                span: agent.span,
            },
            type_params: Vec::new(),
            body: TypeBody::Record(RecordBody {
                fields: vec![RecordField {
                    name: Ident {
                        name: agent.key_name.name.clone(),
                        span: agent.key_name.span,
                    },
                    type_ref: agent.key_type.clone(),
                    refinement: None,
                    init: None,
                    span: agent.key_name.span,
                }],
                span: agent.span,
            }),
            documentation: None,
            span: agent.span,
            trivia: Trivia::default(),
        };
        let mut types_for_handler = resolved_for_handler.types.clone();
        types_for_handler.insert(agent_self_name.clone(), self_decl.clone());
        let local_names_for_handler: std::collections::HashSet<String> =
            types_for_handler.keys().cloned().collect();
        let resolved_for_handler = ResolvedCommons {
            commons: typed.commons.clone(),
            types: types_for_handler,
            fns: typed.fns.clone(),
            methods: typed.methods.clone(),
            local_type_names: local_names_for_handler,
            cross_context: cross_context.clone(),
            agents: table.agents.clone(),
            imported_from: HashMap::new(),
        };
        self_scope.insert(
            "self".to_string(),
            Ty::Named {
                name: agent_self_name.clone(),
                kind: checker::NamedKind::Record,
                args: Vec::new(),
            },
        );
        // v0.81: each `Cell` store field is a bare local of its element type
        // (implicit deref in read position); the `:=` write form is checked
        // separately against `store_cells`.
        for (name, ty) in &store_cells {
            self_scope.insert(name.clone(), ty.clone());
        }
        let _ = key_ty;

        // v0.80/v0.81: invariant well-formedness — predicates are pure `Bool`
        // expressions over the agent's `store` cells (§14, ADR 0108 D5).
        checker::check_invariants(
            &agent.invariants,
            &store_cells,
            &agent.name.name,
            &resolved_for_handler,
            &mut typed.expr_types,
            errors,
            refs,
            hints,
            locals,
            requirements,
        );

        // v0.116 (testing track slice 4): step invariants — predicates over the
        // `old`/`new` state pair, checked against the synthetic state record.
        checker::check_transitions(
            &agent.transitions,
            &state_ty,
            &agent.name.name,
            &resolved_for_handler,
            &mut typed.expr_types,
            errors,
            refs,
            hints,
            locals,
            requirements,
        );

        for handler in &agent.handlers {
            // v0.99 (DECISION H): `by` is a service-edge clause — it establishes
            // the actor (`identity`/`who`) from the inbound request. An agent
            // `on call` handler is reached across the agent boundary by the
            // factory (`__makeAgent`), never from an ingress, so it has no actor
            // and the parser-accepted `by` clause would silently be dropped.
            // Rejecting it turns the deps-split taxonomy's "actor auth never
            // crosses the agent boundary" guarantee into an enforced invariant.
            if let Some(by) = &handler.by_clause {
                errors.push(
                    CompileError::new(
                        "bynk.actor.by_on_agent",
                        by.span,
                        "`by` is a service-edge clause; an agent handler has no actor",
                    )
                    .with_note(
                        "an agent `on call` handler is invoked across the agent boundary, not \
                         from an ingress — remove the `by` clause",
                    ),
                );
            }
            let mut handler_caps: HashMap<String, CapabilityInfo> = HashMap::new();
            for cap_ref in &handler.given {
                if let Some(info) =
                    resolve_given_cap_ref(cap_ref, capability_info_map, cross_context, errors, refs)
                {
                    handler_caps.insert(cap_ref.key().to_string(), info);
                }
            }
            // The handler return type must be Effect[T].
            if !matches!(handler.return_type, TypeRef::Effect(_, _)) {
                errors.push(CompileError::new(
                    "bynk.agent.return_not_effect",
                    handler.return_type.span(),
                    format!(
                        "agent handler must return `Effect[T]`, but got `{}`",
                        ts_type_ref_display(&handler.return_type)
                    ),
                ));
            }
            checker::check_handler_body(
                &resolved_for_handler,
                checker::HandlerBodyCheck {
                    capabilities: handler_caps,
                    declared_capabilities: capability_info_map.clone(),
                    agent_state_ty: Some(state_ty.clone()),
                    agent_self_scope: Some(self_scope.clone()),
                    given_anchor: Some(handler.return_type.span()),
                    report_unused: true,
                    store_cells: store_cells.clone(),
                    store_maps: store_maps.clone(),
                    store_sets: store_sets.clone(),
                    store_caches: store_caches.clone(),
                    store_logs: store_logs.clone(),
                    ..checker::HandlerBodyCheck::new(
                        &handler.body,
                        &handler.return_type,
                        &handler.params,
                        &handler.given,
                    )
                },
                checker::CheckSinks {
                    expr_types: &mut typed.expr_types,
                    errors,
                    refs,
                    hints,
                    locals,
                    requirements,
                },
            );
        }
    }
}

/// Structural equality for TypeRef, used by v0.5 capability/provider signature
/// matching. Doesn't resolve names — it compares the surface syntax. Named
/// types match by their literal identifier; built-ins match by variant.
pub(crate) fn type_refs_match(a: &TypeRef, b: &TypeRef) -> bool {
    match (a, b) {
        (TypeRef::Base(x, _), TypeRef::Base(y, _)) => x == y,
        (TypeRef::Named(x), TypeRef::Named(y)) => x.name == y.name,
        (TypeRef::Result(t1, e1, _), TypeRef::Result(t2, e2, _)) => {
            type_refs_match(t1, t2) && type_refs_match(e1, e2)
        }
        (TypeRef::Option(t1, _), TypeRef::Option(t2, _)) => type_refs_match(t1, t2),
        (TypeRef::Effect(t1, _), TypeRef::Effect(t2, _)) => type_refs_match(t1, t2),
        (TypeRef::HttpResult(t1, _), TypeRef::HttpResult(t2, _)) => type_refs_match(t1, t2),
        (TypeRef::ValidationError(_), TypeRef::ValidationError(_)) => true,
        (TypeRef::JsonError(_), TypeRef::JsonError(_)) => true,
        (TypeRef::Unit(_), TypeRef::Unit(_)) => true,
        _ => false,
    }
}

/// Validate a service's `cors { }` policy (v0.131, ADR 0159). The grammar is
/// lenient — any `name: value` field parses — so the checker is where the closed
/// field set, the value shapes, and the spec-mandated wildcard/credentials
/// constraint (DECISION F) are enforced.
fn validate_cors_policy(
    service: &ServiceDecl,
    policy: &CorsPolicy,
    errors: &mut Vec<CompileError>,
) {
    // CORS is a browser-facing HTTP concern; it is meaningless on any other
    // protocol.
    if !matches!(service.protocol, ServiceProtocol::Http) {
        errors.push(
            CompileError::new(
                "bynk.http.cors_not_http",
                policy.span,
                "a `cors { }` policy is only valid on a `from http` service",
            )
            .with_note("CORS governs cross-origin browser access, which only the HTTP surface has"),
        );
        return;
    }

    // Field names are a closed set; flag anything else (the parser accepts any
    // name, per the annotation precedent).
    for field in &policy.fields {
        if !matches!(
            field.name.name.as_str(),
            "origins" | "headers" | "credentials" | "maxAge"
        ) {
            errors.push(
                CompileError::new(
                    "bynk.http.cors_unknown_field",
                    field.name.span,
                    format!("unknown `cors` field `{}`", field.name.name),
                )
                .with_note("known fields are `origins`, `headers`, `credentials`, and `maxAge`"),
            );
        }
    }

    // `origins` is required and must be a non-empty list of string literals.
    match policy.field("origins") {
        None => errors.push(CompileError::new(
            "bynk.http.cors_invalid_origins",
            policy.span,
            "a `cors { }` policy must declare `origins` — the allowed origins, or `[\"*\"]`",
        )),
        Some(expr) => match &expr.kind {
            ExprKind::ListLit(items) if !items.is_empty() => {
                for item in items {
                    if !matches!(item.kind, ExprKind::StrLit(_)) {
                        errors.push(CompileError::new(
                            "bynk.http.cors_invalid_origins",
                            item.span,
                            "each `cors` origin must be a string literal (e.g. \"https://app.example.com\" or \"*\")",
                        ));
                    }
                }
            }
            _ => errors.push(CompileError::new(
                "bynk.http.cors_invalid_origins",
                expr.span,
                "`cors` `origins` must be a non-empty list of string literals",
            )),
        },
    }

    // `headers`, when present, is a list of string literals.
    if let Some(expr) = policy.field("headers") {
        let ok = matches!(&expr.kind, ExprKind::ListLit(items)
            if items.iter().all(|i| matches!(i.kind, ExprKind::StrLit(_))));
        if !ok {
            errors.push(CompileError::new(
                "bynk.http.cors_invalid_field",
                expr.span,
                "`cors` `headers` must be a list of string literals",
            ));
        }
    }

    // `credentials`, when present, is a boolean literal.
    if let Some(expr) = policy.field("credentials")
        && !matches!(expr.kind, ExprKind::BoolLit(_))
    {
        errors.push(CompileError::new(
            "bynk.http.cors_invalid_field",
            expr.span,
            "`cors` `credentials` must be `true` or `false`",
        ));
    }

    // `maxAge`, when present, is a `Duration` literal.
    if let Some(expr) = policy.field("maxAge")
        && !matches!(expr.kind, ExprKind::DurationLit { .. })
    {
        errors.push(CompileError::new(
            "bynk.http.cors_invalid_field",
            expr.span,
            "`cors` `maxAge` must be a `Duration` literal (e.g. `1.hours`)",
        ));
    }

    // DECISION F: the Fetch spec forbids `Access-Control-Allow-Credentials: true`
    // with a wildcard origin — the browser rejects it at runtime, so catch it at
    // compile time.
    if policy.credentials() && policy.is_wildcard() {
        errors.push(
            CompileError::new(
                "bynk.http.cors_wildcard_credentials",
                policy.span,
                "`cors` cannot combine `credentials: true` with the wildcard origin `[\"*\"]`",
            )
            .with_note(
                "the Fetch spec forbids credentialed requests against a wildcard origin — \
                 list the exact origins instead",
            ),
        );
    }
}

/// v0.141 (ADR 0164): validate a service's `security { }` policy. Security
/// response headers are wire behaviour of the browser-facing HTTP surface, so the
/// section is only legal on a `from http` service; the field vocabulary is the
/// closed set `hsts`/`nosniff`; `hsts` is a *positive* `Duration` (the same rule
/// `@cache maxAge` uses) and `nosniff` a `Bool`.
fn validate_security_policy(
    service: &ServiceDecl,
    policy: &SecurityPolicy,
    errors: &mut Vec<CompileError>,
) {
    // Security headers are a browser-facing HTTP concern; they are meaningless on
    // any other protocol (mirrors the `cors_not_http` gate).
    if !matches!(service.protocol, ServiceProtocol::Http) {
        errors.push(
            CompileError::new(
                "bynk.http.security_not_http",
                policy.span,
                "a `security { }` policy is only valid on a `from http` service",
            )
            .with_note(
                "security response headers govern the browser-facing HTTP surface, \
                 which only a `from http` service has",
            ),
        );
        return;
    }

    // Field names are a closed set; flag anything else (the parser accepts any
    // name, per the CORS / annotation precedent).
    for field in &policy.fields {
        if !matches!(field.name.name.as_str(), "hsts" | "nosniff") {
            errors.push(
                CompileError::new(
                    "bynk.http.security_unknown_field",
                    field.name.span,
                    format!("unknown `security` field `{}`", field.name.name),
                )
                .with_note("known fields are `hsts` and `nosniff`"),
            );
        }
    }

    // `hsts`, when present, is a *positive* `Duration` literal — HSTS with a
    // zero/negative `max-age` is nonsensical (0 would actively *clear* the pin).
    if let Some(expr) = policy.field("hsts")
        && !matches!(&expr.kind, ExprKind::DurationLit { millis, .. } if *millis > 0)
    {
        errors.push(CompileError::new(
            "bynk.http.security_invalid_field",
            expr.span,
            "`security` `hsts` must be a positive `Duration` literal (e.g. `180.days`)",
        ));
    }

    // `nosniff`, when present, is a boolean literal.
    if let Some(expr) = policy.field("nosniff")
        && !matches!(expr.kind, ExprKind::BoolLit(_))
    {
        errors.push(CompileError::new(
            "bynk.http.security_invalid_field",
            expr.span,
            "`security` `nosniff` must be `true` or `false`",
        ));
    }
}

/// v0.142 (ADR 0165): validate a service's `limits { }` policy. A request-body
/// ceiling is wire behaviour of the HTTP surface, so the section is only legal on
/// a `from http` service; the field vocabulary is the closed set `maxBody`; and
/// `maxBody` is a *positive* `Int` byte count (there is no `Size` literal yet — a
/// `1.mb`-style literal is a named follow-on, so v1 takes an `Int`).
fn validate_limits_policy(
    service: &ServiceDecl,
    policy: &LimitsPolicy,
    errors: &mut Vec<CompileError>,
) {
    // A request-body ceiling is an HTTP-surface concern; it is meaningless on any
    // other protocol (mirrors the `cors_not_http` / `security_not_http` gate).
    if !matches!(service.protocol, ServiceProtocol::Http) {
        errors.push(
            CompileError::new(
                "bynk.http.limits_not_http",
                policy.span,
                "a `limits { }` policy is only valid on a `from http` service",
            )
            .with_note(
                "a request-body size ceiling governs the HTTP surface, \
                 which only a `from http` service has",
            ),
        );
        return;
    }

    // Field names are a closed set; flag anything else (the parser accepts any
    // name, per the CORS / security / annotation precedent).
    for field in &policy.fields {
        if field.name.name != "maxBody" {
            errors.push(
                CompileError::new(
                    "bynk.http.limits_unknown_field",
                    field.name.span,
                    format!("unknown `limits` field `{}`", field.name.name),
                )
                .with_note("the only field is `maxBody`"),
            );
        }
    }

    // `maxBody`, when present, is a *positive* `Int` literal — a byte count. Zero
    // or a negative ceiling is nonsensical (it would reject every request). There
    // is no byte `Size` literal yet, so v1 takes a plain `Int` (ADR 0165
    // DECISION C).
    if let Some(expr) = policy.field("maxBody")
        && !matches!(&expr.kind, ExprKind::IntLit { value: n, .. } if *n > 0)
    {
        errors.push(CompileError::new(
            "bynk.http.limits_invalid_field",
            expr.span,
            "`limits` `maxBody` must be a positive `Int` literal — a byte count (e.g. `1_048_576`)",
        ));
    }
}

/// Validate an `on http METHOD "path"` handler (v0.9 §4.1):
///
/// - Path must start with `/`, must not be `/_bynk/...` (reserved).
/// - Every `:name` segment binds to a handler parameter of the same name.
/// - Every parameter is either a path parameter or named `body`.
/// - Path parameter types are constructible from `String` (`String`, refined
///   `String`, or opaque `String`).
/// - GET / DELETE handlers may not have a `body` parameter.
/// - The handler return type must be `Effect[HttpResult[T]]`.
fn validate_http_handler(
    handler: &Handler,
    method: HttpMethod,
    path: &str,
    types: &HashMap<String, TypeDecl>,
    errors: &mut Vec<CompileError>,
) {
    if !path.starts_with('/') {
        errors.push(CompileError::new(
            "bynk.http.invalid_path",
            handler.span,
            format!("HTTP path `{path}` must start with `/`"),
        ));
    }
    if path.starts_with("/_bynk/") || path == "/_bynk" {
        errors.push(
            CompileError::new(
                "bynk.http.reserved_prefix",
                handler.span,
                format!("HTTP path `{path}` uses the reserved `/_bynk/` prefix",),
            )
            .with_note("paths under `/_bynk/` are reserved for internal Bynk dispatch"),
        );
    }
    // Parse segments and collect path-parameter names.
    let mut path_param_names: Vec<&str> = Vec::new();
    for seg in path.split('/').filter(|s| !s.is_empty()) {
        if let Some(rest) = seg.strip_prefix(':') {
            if rest.is_empty() {
                errors.push(CompileError::new(
                    "bynk.http.invalid_path",
                    handler.span,
                    format!("HTTP path `{path}` has an empty parameter segment `:`"),
                ));
            } else {
                path_param_names.push(rest);
            }
        }
    }
    // Every :name must have a matching handler parameter.
    for name in &path_param_names {
        if !handler.params.iter().any(|p| p.name.name == *name) {
            errors.push(CompileError::new(
                "bynk.http.unbound_path_param",
                handler.span,
                format!("path parameter `:{name}` has no matching handler parameter `{name}`",),
            ));
        }
    }
    // Every handler parameter must be either a path param or `body`.
    for p in &handler.params {
        let is_path = path_param_names.iter().any(|n| n == &p.name.name.as_str());
        let is_body = p.name.name == "body";
        if !is_path && !is_body {
            errors.push(
                CompileError::new(
                    "bynk.http.extra_param",
                    p.span,
                    format!(
                        "handler parameter `{}` is not a path parameter and is not named `body`",
                        p.name.name
                    ),
                )
                .with_note(
                    "HTTP handler parameters must either match a `:name` path segment or be named `body`",
                ),
            );
        }
        // Path params must be constructible from String.
        if is_path && !is_string_constructible(&p.type_ref, types) {
            errors.push(
                CompileError::new(
                    "bynk.http.path_param_not_stringy",
                    p.type_ref.span(),
                    format!(
                        "path parameter `{}` must have a type constructible from `String` (got `{}`)",
                        p.name.name,
                        ts_type_ref_display(&p.type_ref),
                    ),
                )
                .with_note(
                    "use `String`, a refined `String`, or an opaque type whose base is `String`",
                ),
            );
        }
        if is_body && method.forbids_body() {
            errors.push(
                CompileError::new(
                    "bynk.http.body_on_get_or_delete",
                    p.span,
                    format!(
                        "`on http {}` handlers may not declare a `body` parameter",
                        method.as_str()
                    ),
                )
                .with_note("GET and DELETE requests conventionally carry no body in Bynk v0.9"),
            );
        }
    }
    // Validate return type shape.
    let return_ok = match &handler.return_type {
        TypeRef::Effect(inner, _) => matches!(inner.as_ref(), TypeRef::HttpResult(_, _)),
        _ => false,
    };
    if !return_ok {
        errors.push(CompileError::new(
            "bynk.http.return_not_effect_http_result",
            handler.return_type.span(),
            format!(
                "`on http` handler must return `Effect[HttpResult[T]]`, but got `{}`",
                ts_type_ref_display(&handler.return_type),
            ),
        ));
    }
}

/// Validate a handler's handler-position annotations (v0.140, ADR 0163). The one
/// handler annotation is `@cache(maxAge: <Duration>, scope: public|private)`,
/// legal solely on an `on http GET` handler. This runs for *every* handler —
/// services and agents — so a misplaced `@cache` (a non-GET route, another
/// protocol, or an agent handler) is caught wherever it is written, and an
/// unknown annotation name is flagged rather than silently ignored. The
/// automatic conditional `ETag`/`304` half carries no author surface, so `@cache`
/// is the only annotation validated here.
fn validate_handler_annotations(handler: &Handler, errors: &mut Vec<CompileError>) {
    let is_get = matches!(
        handler.kind,
        HandlerKind::Http {
            method: HttpMethod::Get,
            ..
        }
    );
    // v0.142 (ADR 0165): `@limit` is the inverse of `@cache` — it caps a request
    // body, so it is valid only on a body-taking route (POST/PUT/PATCH); a GET or
    // DELETE (and any non-HTTP handler) has no body to limit.
    let is_body_method = matches!(
        handler.kind,
        HandlerKind::Http {
            method: HttpMethod::Post | HttpMethod::Put | HttpMethod::Patch,
            ..
        }
    );
    let mut seen_cache = false;
    let mut seen_limit = false;
    for ann in &handler.annotations {
        match ann.name.name.as_str() {
            "cache" => {
                if seen_cache {
                    errors.push(CompileError::new(
                        "bynk.http.cache_duplicate",
                        ann.span,
                        "a handler carries at most one `@cache` annotation",
                    ));
                    continue;
                }
                seen_cache = true;
                if !is_get {
                    errors.push(
                        CompileError::new(
                            "bynk.http.cache_on_non_get",
                            ann.span,
                            "`@cache` is only valid on an `on http GET` handler",
                        )
                        .with_note(
                            "conditional caching applies to safe, idempotent reads — a `GET` route",
                        ),
                    );
                    continue;
                }
                validate_cache_args(ann, errors);
            }
            "limit" => {
                if seen_limit {
                    errors.push(CompileError::new(
                        "bynk.http.limit_duplicate",
                        ann.span,
                        "a handler carries at most one `@limit` annotation",
                    ));
                    continue;
                }
                seen_limit = true;
                if !is_body_method {
                    errors.push(
                        CompileError::new(
                            "bynk.http.limit_on_bodyless",
                            ann.span,
                            "`@limit` is only valid on a body-taking `on http` route (POST/PUT/PATCH)",
                        )
                        .with_note(
                            "a request-body size cap applies to routes that read a body — a GET or DELETE has none",
                        ),
                    );
                    continue;
                }
                validate_limit_args(ann, errors);
            }
            other => {
                errors.push(
                    CompileError::new(
                        "bynk.http.unknown_handler_annotation",
                        ann.name.span,
                        format!(
                            "unknown handler annotation `@{other}` — the handler annotations are `@cache` and `@limit`"
                        ),
                    )
                    .with_note("handler annotations are a closed set (ADR 0163, ADR 0165)"),
                );
            }
        }
    }
}

/// Validate `@cache`'s arguments on a GET handler (v0.140, ADR 0163): a required
/// `maxAge:` positive `Duration` literal (the freshness window — the one thing the
/// compiler cannot derive) and an optional `scope:` of `public`/`private`
/// (defaulting to `private` at emit time). Any other argument — a stray label or a
/// positional value — is a diagnostic; the vocabulary is closed.
fn validate_cache_args(ann: &Annotation, errors: &mut Vec<CompileError>) {
    let mut max_age: Option<&AnnotationArg> = None;
    let mut scope: Option<&AnnotationArg> = None;
    for arg in &ann.args {
        match arg.label.as_ref().map(|l| l.name.as_str()) {
            Some("maxAge") => max_age = Some(arg),
            Some("scope") => scope = Some(arg),
            _ => {
                errors.push(
                    CompileError::new(
                        "bynk.http.cache_unknown_arg",
                        arg.span,
                        "`@cache` accepts only the `maxAge:` and `scope:` arguments",
                    )
                    .with_note("write `@cache(maxAge: 5.minutes, scope: private)`"),
                );
            }
        }
    }
    // `maxAge` is required and must be a *positive* `Duration` literal — the same
    // positive-duration rule the `@ttl` store annotation uses.
    match max_age.map(|a| &a.value.kind) {
        Some(ExprKind::DurationLit { millis, .. }) if *millis > 0 => {}
        Some(_) => {
            errors.push(CompileError::new(
                "bynk.http.cache_bad_max_age",
                max_age.unwrap().span,
                "`@cache` `maxAge` must be a positive `Duration` literal (e.g. `5.minutes`)",
            ));
        }
        None => {
            errors.push(
                CompileError::new(
                    "bynk.http.cache_bad_max_age",
                    ann.span,
                    "`@cache` requires a `maxAge:` argument — the freshness window",
                )
                .with_note(
                    "the `ETag` revalidation is automatic; only the freshness window is declared",
                ),
            );
        }
    }
    // `scope`, when present, is the bare identifier `public` or `private`.
    if let Some(scope) = scope {
        let ok = matches!(
            &scope.value.kind,
            ExprKind::Ident(id) if id.name == "public" || id.name == "private"
        );
        if !ok {
            errors.push(CompileError::new(
                "bynk.http.cache_bad_scope",
                scope.span,
                "`@cache` `scope` must be `public` or `private`",
            ));
        }
    }
}

/// Validate `@limit`'s arguments on a body-taking route (v0.142, ADR 0165): a
/// required `maxBody:` positive `Int` literal — a byte count, the one ceiling only
/// the author knows. Any other argument — a stray label or a positional value — is
/// a diagnostic; the vocabulary is closed. A route `@limit` overrides the service
/// `limits { }` default at emit time. There is no `Size` literal yet, so the byte
/// count is a plain `Int` (DECISION C).
fn validate_limit_args(ann: &Annotation, errors: &mut Vec<CompileError>) {
    let mut max_body: Option<&AnnotationArg> = None;
    for arg in &ann.args {
        match arg.label.as_ref().map(|l| l.name.as_str()) {
            Some("maxBody") => max_body = Some(arg),
            _ => {
                errors.push(
                    CompileError::new(
                        "bynk.http.limit_unknown_arg",
                        arg.span,
                        "`@limit` accepts only the `maxBody:` argument",
                    )
                    .with_note("write `@limit(maxBody: 26_214_400)`"),
                );
            }
        }
    }
    // `maxBody` is required and must be a *positive* `Int` literal — a byte count.
    match max_body.map(|a| &a.value.kind) {
        Some(ExprKind::IntLit { value: n, .. }) if *n > 0 => {}
        Some(_) => {
            errors.push(CompileError::new(
                "bynk.http.limit_bad_max_body",
                max_body.unwrap().span,
                "`@limit` `maxBody` must be a positive `Int` literal — a byte count (e.g. `26_214_400`)",
            ));
        }
        None => {
            errors.push(
                CompileError::new(
                    "bynk.http.limit_bad_max_body",
                    ann.span,
                    "`@limit` requires a `maxBody:` argument — the byte ceiling",
                )
                .with_note(
                    "the ceiling is a policy the compiler cannot derive; only the author knows it",
                ),
            );
        }
    }
}

/// Validate an `on cron "expr" (at: Int?) -> Effect[Result[(), E]]` handler
/// (v0.10a §4.1): at most one `Int` parameter (the scheduled time, Unix epoch
/// milliseconds), a structurally well-formed schedule, and the unit-Result
/// return shape. The service-only rule is enforced earlier, in the parser
/// (`bynk.parse.cron_in_agent`).
fn validate_cron_handler(handler: &Handler, expr: &str, errors: &mut Vec<CompileError>) {
    // A cron handler takes at most one parameter — the scheduled time, typed
    // `Int` (epoch milliseconds). A scheduled trigger has no other payload.
    if handler.params.len() > 1 {
        errors.push(
            CompileError::new(
                "bynk.cron.bad_params",
                handler.params[1].span,
                "`on cron` handlers take at most one parameter (the scheduled time)",
            )
            .with_note("a scheduled trigger's only input is the time it fired"),
        );
    } else if let Some(p) = handler.params.first()
        && !matches!(p.type_ref, TypeRef::Base(BaseType::Int, _))
    {
        errors.push(
            CompileError::new(
                "bynk.cron.bad_params",
                p.type_ref.span(),
                format!(
                    "an `on cron` parameter must be `Int` (the scheduled time in epoch milliseconds), got `{}`",
                    ts_type_ref_display(&p.type_ref),
                ),
            )
            .with_note("wrap it in your own time type inside the body if you want stronger typing"),
        );
    }
    // The schedule must be five whitespace-separated fields (light structural
    // check; per-field validation is deferred — v0.10 §4.1, [DECISION 4]).
    let fields = expr.split_whitespace().count();
    if fields != 5 {
        errors.push(
            CompileError::new(
                "bynk.cron.invalid_schedule",
                handler.span,
                format!(
                    "cron expression `{expr}` must have exactly five whitespace-separated fields (got {fields})",
                ),
            )
            .with_note("the fields are: minute hour day-of-month month day-of-week"),
        );
    }
    // The return type must be `Effect[Result[(), E]]`.
    let return_ok = match &handler.return_type {
        TypeRef::Effect(inner, _) => match inner.as_ref() {
            TypeRef::Result(ok, _err, _) => matches!(ok.as_ref(), TypeRef::Unit(_)),
            _ => false,
        },
        _ => false,
    };
    if !return_ok {
        errors.push(CompileError::new(
            "bynk.cron.return_not_effect_result",
            handler.return_type.span(),
            format!(
                "`on cron` handler must return `Effect[Result[(), E]]`, but got `{}`",
                ts_type_ref_display(&handler.return_type),
            ),
        ));
    }
}

/// Validate an `on queue "name" (message: T) -> Effect[Result[(), E]]` handler
/// (v0.10b §4.2): a non-empty queue name, exactly one parameter (the message,
/// any wire-deserialisable type), and the unit-Result return shape. `Ok(())`
/// acknowledges the message at emission; `Err` retries it. The service-only
/// rule is enforced earlier, in the parser (`bynk.parse.queue_in_agent`).
fn validate_queue_handler(handler: &Handler, name: &str, errors: &mut Vec<CompileError>) {
    if name.is_empty() {
        errors.push(CompileError::new(
            "bynk.queue.invalid_name",
            handler.span,
            "`on queue` requires a non-empty queue name",
        ));
    }
    // Exactly one parameter — the message. (Conventionally named `message`.)
    if handler.params.len() != 1 {
        errors.push(
            CompileError::new(
                "bynk.queue.bad_params",
                handler.span,
                format!(
                    "`on message` handlers take exactly one parameter (the message), got {}",
                    handler.params.len(),
                ),
            )
            .with_note("a queue consumer processes one message per invocation"),
        );
    }
    // v0.44: the return type must be `Effect[QueueResult]` (the verdict sum).
    let return_ok = match &handler.return_type {
        TypeRef::Effect(inner, _) => matches!(inner.as_ref(), TypeRef::QueueResult(_)),
        _ => false,
    };
    if !return_ok {
        errors.push(CompileError::new(
            "bynk.queue.return_not_queue_result",
            handler.return_type.span(),
            format!(
                "`on message` handler must return `Effect[QueueResult]`, but got `{}`",
                ts_type_ref_display(&handler.return_type),
            ),
        ));
    }
}

/// True when `r` resolves to `String`, a refined-base `String`, or an
/// opaque-base `String`. v0.9 path parameter requirement.
fn is_string_constructible(r: &TypeRef, types: &HashMap<String, TypeDecl>) -> bool {
    match r {
        TypeRef::Base(BaseType::String, _) => true,
        TypeRef::Named(id) => match types.get(&id.name).map(|t| &t.body) {
            Some(TypeBody::Refined { base, .. }) => *base == BaseType::String,
            Some(TypeBody::Opaque { base, .. }) => *base == BaseType::String,
            _ => false,
        },
        _ => false,
    }
}

/// v0.20a: function types are confined to non-boundary positions — fn/lambda
/// parameters, returns, and locals. Walk a type reference and reject any
/// function type found in a position that would serialise, persist, or cross
/// a boundary (`bynk.types.function_at_boundary`).
/// v0.102 (§2.9): true if a type *is or wraps* a held resource (`Connection`),
/// looking through `Option`/`Effect` — the shapes a held value legitimately
/// takes: an `Option[Connection]` cell value, an `Effect[Connection]` capability
/// return, a bare `Connection` handler parameter.
pub(crate) fn type_ref_is_held(r: &TypeRef) -> bool {
    match r {
        TypeRef::Connection(..) => true,
        TypeRef::Option(inner, _) | TypeRef::Effect(inner, _) => type_ref_is_held(inner),
        _ => false,
    }
}

/// v0.102 (§2.9.3): validate one agent `store` field's value types, applying the
/// held-resource storage rules. Held values are admitted in
/// `Cell[Option[Connection]]` / `Map[K, Connection]` (an exception to the
/// serialisable-value rule — hibernation preserves them, not JSON), and rejected
/// in `Set`/`Log`/`Cache`. Non-held value types fall through to the ordinary
/// boundary check.
fn validate_store_field_value_types(
    f: &StoreField,
    types: &std::collections::HashMap<String, TypeDecl>,
    errors: &mut Vec<CompileError>,
) {
    let head = f.kind.head.name.as_str();
    let reject_held_storage = |span: Span, errors: &mut Vec<CompileError>| {
        errors.push(
            CompileError::new(
                "bynk.held.unsupported_storage",
                span,
                format!(
                    "a held value cannot be stored in a `{head}` — held resources may only live in `Cell[Option[Connection]]` or `Map[K, Connection]` (§2.9.3)"
                ),
            )
            .with_note(
                "`Set` needs value-equality, and `Log`/`Cache` would retain or evict a held resource without disposing it",
            ),
        );
    };
    match head {
        // The value position(s) where a held resource is admitted.
        "Cell" => match f.kind.args.first() {
            Some(v) if type_ref_is_held(v) => {} // admitted
            Some(v) => reject_fn_types(v, "an agent store field", types, errors),
            None => {}
        },
        "Map" => match f.kind.args.as_slice() {
            [k, v] => {
                reject_fn_types(k, "an agent store field", types, errors); // key
                if !type_ref_is_held(v) {
                    reject_fn_types(v, "an agent store field", types, errors);
                }
            }
            args => {
                for arg in args {
                    reject_fn_types(arg, "an agent store field", types, errors);
                }
            }
        },
        // Kinds that reject held values outright.
        "Set" | "Cache" | "Log" => {
            for arg in &f.kind.args {
                if type_ref_is_held(arg) {
                    reject_held_storage(arg.span(), errors);
                } else {
                    reject_fn_types(arg, "an agent store field", types, errors);
                }
            }
        }
        _ => {
            for arg in &f.kind.args {
                reject_fn_types(arg, "an agent store field", types, errors);
            }
        }
    }
}

fn reject_fn_types(
    r: &TypeRef,
    what: &str,
    types: &std::collections::HashMap<String, TypeDecl>,
    errors: &mut Vec<CompileError>,
) {
    match r {
        TypeRef::Fn(_, _, span) => {
            errors.push(
                CompileError::new(
                    "bynk.types.function_at_boundary",
                    *span,
                    format!(
                        "a function type cannot appear in {what} — functions cannot serialise or cross a boundary"
                    ),
                )
                .with_note(
                    "function types are confined to fn/lambda parameters, returns, and locals",
                ),
            );
        }
        // v0.91 (ADR 0115 D2): a `Query[T]` is non-storable and non-boundary —
        // built, passed within an agent, and executed, never persisted or sent.
        TypeRef::Query(_, span) => {
            errors.push(
                CompileError::new(
                    "bynk.types.query_at_boundary",
                    *span,
                    format!(
                        "a `Query` type cannot appear in {what} — a query is built and executed in place, never persisted or sent across a boundary"
                    ),
                )
                .with_note(
                    "terminate the query (`.collect`/`.first`/…) and store or send the result instead",
                ),
            );
        }
        // v0.100: a `Stream[T]` is non-storable and non-boundary — a live
        // value-over-time source, built and consumed in place, never persisted
        // or sent.
        TypeRef::Stream(_, span) => {
            errors.push(
                CompileError::new(
                    "bynk.types.stream_at_boundary",
                    *span,
                    format!(
                        "a `Stream` type cannot appear in {what} — a stream is a live value-over-time source, never persisted or sent across a boundary"
                    ),
                )
                .with_note(
                    "drain the stream (`.collect()`) and store or send the resulting `List` instead",
                ),
            );
        }
        // v0.102: a `Connection[F]` (a held resource) is non-boundary — built
        // and disposed in place under the linearity discipline, never persisted
        // or sent across a boundary.
        TypeRef::Connection(_, span) => {
            errors.push(
                CompileError::new(
                    "bynk.types.held_at_boundary",
                    *span,
                    format!(
                        "a `Connection` type cannot appear in {what} — a held resource is built and disposed in place, never persisted or sent across a boundary"
                    ),
                )
                .with_note(
                    "hold the connection in agent state (`Cell[Option[Connection]]` / `Map[K, Connection]`) instead of crossing a boundary with it",
                ),
            );
        }
        // v0.20b: the boundary rule looks through collections — a
        // `List[Int -> Int]` field is still `function_at_boundary`.
        TypeRef::Result(a, b, _) | TypeRef::Map(a, b, _) => {
            reject_fn_types(a, what, types, errors);
            reject_fn_types(b, what, types, errors);
        }
        TypeRef::Option(a, _)
        | TypeRef::Effect(a, _)
        | TypeRef::HttpResult(a, _)
        | TypeRef::List(a, _) => reject_fn_types(a, what, types, errors),
        // v0.119: a `History[Agent]` reaching a declared position is already
        // reported by the resolver (`bynk.history.outside_property`); nothing to
        // add here.
        TypeRef::History(_, _) => {}
        // v0.174 (#592): a generic record instantiation is boundary-serialisable
        // through its monomorphised codec (`serialise_Paginated_User`) — so the
        // application itself is admitted, and the rule instead looks *through* it
        // into the type arguments. A non-serialisable argument (a function, a
        // `Query`, …) is rejected there, with the argument's own boundary error.
        // (ADR 0183 Decision C's blanket `generic_record_at_boundary` rejection
        // was the previous behaviour.) A *recursive* generic record — one that
        // transitively contains itself, through any wrapper or generic argument —
        // has no finite set of monomorphised codecs, so it is still rejected
        // here (the resolver's `recursive_record_field` guard only catches a
        // direct self-edge, not recursion through an `Option`/`List` wrapper).
        TypeRef::App { name, args, span } => {
            if generic_record_is_recursive(&name.name, types) {
                errors.push(
                    CompileError::new(
                        "bynk.generics.recursive_generic_at_boundary",
                        *span,
                        format!(
                            "recursive generic record `{}` cannot appear in {what} — it has no finite monomorphised codec",
                            name.name
                        ),
                    )
                    .with_note(
                        "a generic record that transitively contains itself is not yet \
                         boundary-serialisable; use a concrete (non-generic) recursive type, \
                         or break the cycle",
                    ),
                );
            }
            for a in args {
                reject_fn_types(a, what, types, errors);
            }
        }
        TypeRef::Base(..)
        | TypeRef::Named(_)
        | TypeRef::QueueResult(_)
        | TypeRef::ValidationError(_)
        | TypeRef::JsonError(_)
        | TypeRef::Unit(_) => {}
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
        attributed.extend(
            file_errors
                .into_iter()
                .map(|e| (pf.identity_path.clone(), e)),
        );
    }
    attributed
}

/// v0.174 (#592): a `name -> TypeDecl` table over a set of items, for the
/// recursive-generic boundary walk.
pub(crate) fn collect_type_decls<'a>(
    items: impl Iterator<Item = &'a CommonsItem>,
) -> std::collections::HashMap<String, TypeDecl> {
    let mut out = std::collections::HashMap::new();
    for item in items {
        if let CommonsItem::Type(t) = item {
            out.entry(t.name.name.clone()).or_insert_with(|| t.clone());
        }
    }
    out
}

/// Item-level body of the boundary confinement, shared with the single-file
/// (legacy) compile path in `bynkc`'s `lib.rs`.
pub fn check_function_type_boundary_items(
    items: &[CommonsItem],
    types: &std::collections::HashMap<String, TypeDecl>,
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
