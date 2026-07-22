//! message-bundles slice 3 (#878): the ICU MessageFormat placeholder
//! mini-parser. Self-contained and `bynk-syntax`-free by design (Decision B) —
//! the whole ICU sub-grammar (`{name, plural, one {…} other {…}}`,
//! `{name, select, …}`, `{name, number[, style]}`, `{name, date[, style]}`)
//! lives entirely inside a `messages` template's `String` content, parsed
//! here as plain `&str` and consumed by both the checker
//! (`bynk-emit/src/project/validate.rs`) and the emitter
//! (`emit_message_entry_renderer`). No `bynk-syntax` grammar/lexer/AST
//! change backs this — a template stays one opaque `String` all the way
//! through parsing.
//!
//! Quoting: a bare `'` toggles a "quoted" region; `''` inside either mode
//! means a literal `'` and doesn't toggle; while quoted, `{`/`}`/`,`/`#` are
//! inert literal text. This is a deliberately narrower rule than full ICU
//! MessageFormat's own quoting semantics — sufficient for this slice's fixed
//! subset, not a general ICU implementation (Decision B/the proposal's named
//! scope cuts).
//!
//! Explicitly unsupported, each with its own diagnosable
//! [`IcuParseErrorKind`] rather than silent misbehaviour: `selectordinal`,
//! `plural`'s `offset:`/`=N` exact-value arms, arbitrary CLDR skeletons
//! beyond the fixed style keywords below, and nesting a second `{arg, …}`
//! dispatch inside a sub-message (a sub-message is literal text + `#` only).

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IcuPlaceholder<'a> {
    pub(crate) name: &'a str,
    pub(crate) kind: PlaceholderKind<'a>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlaceholderKind<'a> {
    Plural {
        arms: Vec<(PluralCategory, Vec<SubSegment>)>,
    },
    Select {
        arms: Vec<(&'a str, Vec<SubSegment>)>,
    },
    Number {
        style: Option<NumberStyle>,
    },
    Date {
        style: Option<DateStyle>,
    },
}

/// The coarse comparison unit for cross-locale format agreement
/// (`bynk.messages.format_mismatch`, `bynk-emit/src/project/validate.rs`) —
/// arm/style content doesn't matter for agreement, only which of the five
/// surface forms a placeholder uses. `Plain` covers the bare `{name}` fast
/// path, which never calls into this parser at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum FormatKind {
    Plain,
    Plural,
    Select,
    Number,
    Date,
}

impl FormatKind {
    /// PR #879 review (finding 2): the lowercase surface vocabulary used
    /// everywhere else (the diagnostics registry, ICU keywords themselves,
    /// `message_template_placeholder_summary`) — not `{:?}`'s capitalized
    /// Rust enum name, which leaked into `bynk.messages.format_mismatch`'s
    /// message text.
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::Plural => "plural",
            Self::Select => "select",
            Self::Number => "number",
            Self::Date => "date",
        }
    }
}

impl<'a> PlaceholderKind<'a> {
    pub(crate) fn format_kind(&self) -> FormatKind {
        match self {
            PlaceholderKind::Plural { .. } => FormatKind::Plural,
            PlaceholderKind::Select { .. } => FormatKind::Select,
            PlaceholderKind::Number { .. } => FormatKind::Number,
            PlaceholderKind::Date { .. } => FormatKind::Date,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PluralCategory {
    Zero,
    One,
    Two,
    Few,
    Many,
    Other,
}

impl PluralCategory {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "zero" => Some(Self::Zero),
            "one" => Some(Self::One),
            "two" => Some(Self::Two),
            "few" => Some(Self::Few),
            "many" => Some(Self::Many),
            "other" => Some(Self::Other),
            _ => None,
        }
    }

    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Zero => "zero",
            Self::One => "one",
            Self::Two => "two",
            Self::Few => "few",
            Self::Many => "many",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NumberStyle {
    Integer,
    Percent,
}

impl NumberStyle {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Integer => "integer",
            Self::Percent => "percent",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DateStyle {
    Short,
    Medium,
    Long,
    Full,
}

impl DateStyle {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Short => "short",
            Self::Medium => "medium",
            Self::Long => "long",
            Self::Full => "full",
        }
    }
}

/// One piece of a `plural`/`select` arm's sub-message. Owned, not `&'a str`:
/// ICU `''`-unescaping can shrink byte length, so a literal sub-segment
/// cannot always borrow from the source template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SubSegment {
    Literal(String),
    /// Only valid inside a `plural` arm — substitutes the argument's own
    /// value, run through `formatIcuNumber`.
    Hash,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IcuParseError {
    /// Byte offset into the placeholder's `inner` text (the content between
    /// the outer `{`/`}`, NOT the outer template) — rebased by the caller
    /// against `MessageEntry::template_span` (message-bundles slice 3
    /// Decision C).
    pub(crate) offset: usize,
    pub(crate) len: usize,
    pub(crate) kind: IcuParseErrorKind,
}

impl IcuParseError {
    fn at(offset: usize, len: usize, kind: IcuParseErrorKind) -> Self {
        Self {
            offset,
            len: len.max(1),
            kind,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum IcuParseErrorKind {
    EmptyPlaceholderName,
    MissingFormatKeyword,
    UnknownFormatKeyword(String),
    UnsupportedSelectordinal,
    UnsupportedPluralOffset,
    UnsupportedPluralExactValueArm(String),
    UnknownStyleKeyword {
        construct: &'static str,
        found: String,
    },
    TrailingContentAfterStyle(String),
    UnknownPluralCategory(String),
    DuplicateArmKey(String),
    MissingArmBody(String),
    MissingOtherArm,
    HashOutsidePluralArm,
    NestedDispatchInSubMessage,
    DanglingQuoteEscape,
    UnbalancedArmBraces,
}

impl IcuParseErrorKind {
    pub(crate) fn message(&self) -> String {
        match self {
            Self::EmptyPlaceholderName => "a message placeholder has no argument name before its first `,`".to_string(),
            Self::MissingFormatKeyword => {
                "a message placeholder with a `,` needs a format keyword (`plural`, `select`, `number`, or `date`) after the argument name".to_string()
            }
            Self::UnknownFormatKeyword(kw) => format!(
                "unknown format keyword \"{kw}\"; expected `plural`, `select`, `number`, or `date`"
            ),
            Self::UnsupportedSelectordinal => {
                "`selectordinal` is not supported; only `plural`, `select`, `number`, and `date` are".to_string()
            }
            Self::UnsupportedPluralOffset => {
                "a `plural` placeholder's `offset:` is not supported".to_string()
            }
            Self::UnsupportedPluralExactValueArm(key) => format!(
                "a `plural` placeholder's exact-value arm \"{key}\" is not supported; use a CLDR category (`zero`/`one`/`two`/`few`/`many`/`other`)"
            ),
            Self::UnknownStyleKeyword { construct, found } => format!(
                "unknown `{construct}` style \"{found}\""
            ),
            Self::TrailingContentAfterStyle(found) => format!(
                "unexpected content \"{found}\" after the style keyword"
            ),
            Self::UnknownPluralCategory(key) => format!(
                "unknown plural category \"{key}\"; expected one of zero, one, two, few, many, other"
            ),
            Self::DuplicateArmKey(key) => format!(
                "arm \"{key}\" is already declared in this placeholder"
            ),
            Self::MissingArmBody(key) => {
                if key.is_empty() {
                    "expected an arm body (`{…}`)".to_string()
                } else {
                    format!("arm \"{key}\" is missing its `{{…}}` body")
                }
            }
            Self::MissingOtherArm => {
                "a `plural`/`select` placeholder must declare an `other` arm".to_string()
            }
            Self::HashOutsidePluralArm => {
                "`#` is only valid inside a `plural` arm's sub-message".to_string()
            }
            Self::NestedDispatchInSubMessage => {
                "a sub-message cannot contain another `{…}` placeholder; only literal text and `#` are allowed"
                    .to_string()
            }
            Self::DanglingQuoteEscape => {
                "an unterminated `'` quote in this template".to_string()
            }
            Self::UnbalancedArmBraces => "an arm's `{…}` body is never closed".to_string(),
        }
    }
}

/// Scans `s` quote-aware (bare `'` toggles quoting, `''` is a literal `'`),
/// tracking brace depth (unquoted `{`/`}` adjust it, starting from 0), and
/// returns the byte offset of the first unquoted `,` found at depth 0 — the
/// one primitive shared by every top-level split this parser needs (name vs.
/// the rest; format keyword vs. its arms/style).
fn find_top_level_comma(s: &str) -> Option<usize> {
    let mut depth: i32 = 0;
    let mut quoted = false;
    let mut chars = s.char_indices().peekable();
    while let Some((idx, ch)) = chars.next() {
        match ch {
            '\'' => {
                if chars.peek().is_some_and(|&(_, c)| c == '\'') {
                    chars.next();
                } else {
                    quoted = !quoted;
                }
            }
            '{' if !quoted => depth += 1,
            '}' if !quoted => depth -= 1,
            ',' if !quoted && depth == 0 => return Some(idx),
            _ => {}
        }
    }
    None
}

/// Quote+depth-aware scan from just after a placeholder's opening `{`
/// (`rest`), used only once emission-side detection (`split_template`) has
/// already decided this is an ICU-dispatch placeholder (a `,` precedes the
/// naive first `}`). Depth starts at 1 for the still-open outer brace;
/// returns the byte offset of the true closing `}` (depth reaching 0),
/// `None` if it never closes.
pub(crate) fn find_icu_close(rest: &str) -> Option<usize> {
    let mut depth: i32 = 1;
    let mut quoted = false;
    let mut chars = rest.char_indices().peekable();
    while let Some((idx, ch)) = chars.next() {
        match ch {
            '\'' => {
                if chars.peek().is_some_and(|&(_, c)| c == '\'') {
                    chars.next();
                } else {
                    quoted = !quoted;
                }
            }
            '{' if !quoted => depth += 1,
            '}' if !quoted => {
                depth -= 1;
                if depth == 0 {
                    return Some(idx);
                }
            }
            _ => {}
        }
    }
    None
}

/// Parses one arm-list (`plural`'s or `select`'s arms), starting at `text`
/// (a slice of the placeholder's `inner`, already past the format keyword's
/// own comma) whose absolute offset within `inner` is `base_offset` — every
/// error this returns carries an offset already rebased to `inner`, not
/// `text`. `allow_hash` gates whether a bare `#` is legal in an arm body
/// (`plural` only); `is_plural` gates category-vocabulary checking and the
/// `offset:`/`=N` exact-value rejections (`select` allows arbitrary keys).
fn parse_arms(
    text: &str,
    base_offset: usize,
    allow_hash: bool,
    is_plural: bool,
) -> Result<Vec<(String, Vec<SubSegment>)>, IcuParseError> {
    let mut arms = Vec::new();
    let mut i = 0usize;
    loop {
        while i < text.len() && text.as_bytes()[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= text.len() {
            break;
        }
        let key_start = i;
        while i < text.len() {
            let c = text[i..].chars().next().expect("i < text.len()");
            if c.is_whitespace() || c == '{' {
                break;
            }
            i += c.len_utf8();
        }
        let key = &text[key_start..i];
        if key.is_empty() {
            return Err(IcuParseError::at(
                base_offset + i,
                1,
                IcuParseErrorKind::MissingArmBody(String::new()),
            ));
        }
        if is_plural && key.starts_with("offset:") {
            return Err(IcuParseError::at(
                base_offset + key_start,
                key.len(),
                IcuParseErrorKind::UnsupportedPluralOffset,
            ));
        }
        if is_plural && key.starts_with('=') {
            return Err(IcuParseError::at(
                base_offset + key_start,
                key.len(),
                IcuParseErrorKind::UnsupportedPluralExactValueArm(key.to_string()),
            ));
        }
        if is_plural && PluralCategory::parse(key).is_none() {
            return Err(IcuParseError::at(
                base_offset + key_start,
                key.len(),
                IcuParseErrorKind::UnknownPluralCategory(key.to_string()),
            ));
        }
        // PR #879 review (finding 1): a repeated arm key parses fine but
        // emits a duplicate-property object literal (`{ "one": ..., "one":
        // ... }`), which `tsc --strict` rejects (TS1117) — a generated-code
        // failure the author never sees as a Bynk diagnostic. Caught here,
        // before the body is even parsed, so it's reported at the key
        // itself like every other arm-key error.
        if arms.iter().any(|(k, _)| k == key) {
            return Err(IcuParseError::at(
                base_offset + key_start,
                key.len(),
                IcuParseErrorKind::DuplicateArmKey(key.to_string()),
            ));
        }
        while i < text.len() && text.as_bytes()[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= text.len() || text.as_bytes()[i] != b'{' {
            return Err(IcuParseError::at(
                base_offset + i,
                1,
                IcuParseErrorKind::MissingArmBody(key.to_string()),
            ));
        }
        i += 1; // consume the arm's opening '{'
        let body_start = i;
        let (segs, consumed) =
            parse_sub_message(&text[body_start..], base_offset + body_start, allow_hash)?;
        arms.push((key.to_string(), segs));
        i = body_start + consumed;
    }
    Ok(arms)
}

/// Parses one arm's sub-message body starting right after its opening `{`
/// (`s`), quote-aware, stopping at the first unquoted `}`. Returns the
/// parsed segments plus the number of bytes of `s` consumed, *including*
/// that closing brace.
fn parse_sub_message(
    s: &str,
    base_offset: usize,
    allow_hash: bool,
) -> Result<(Vec<SubSegment>, usize), IcuParseError> {
    let mut segs = Vec::new();
    let mut literal = String::new();
    let mut quoted = false;
    let mut chars = s.char_indices().peekable();
    while let Some((idx, ch)) = chars.next() {
        match ch {
            '\'' => {
                if chars.peek().is_some_and(|&(_, c)| c == '\'') {
                    chars.next();
                    literal.push('\'');
                } else {
                    quoted = !quoted;
                }
            }
            '{' if !quoted => {
                return Err(IcuParseError::at(
                    base_offset + idx,
                    1,
                    IcuParseErrorKind::NestedDispatchInSubMessage,
                ));
            }
            '}' if !quoted => {
                if !literal.is_empty() {
                    segs.push(SubSegment::Literal(std::mem::take(&mut literal)));
                }
                return Ok((segs, idx + 1));
            }
            '#' if !quoted && allow_hash => {
                if !literal.is_empty() {
                    segs.push(SubSegment::Literal(std::mem::take(&mut literal)));
                }
                segs.push(SubSegment::Hash);
            }
            '#' if !quoted => {
                return Err(IcuParseError::at(
                    base_offset + idx,
                    1,
                    IcuParseErrorKind::HashOutsidePluralArm,
                ));
            }
            other => literal.push(other),
        }
    }
    if quoted {
        Err(IcuParseError::at(
            base_offset + s.len(),
            1,
            IcuParseErrorKind::DanglingQuoteEscape,
        ))
    } else {
        Err(IcuParseError::at(
            base_offset,
            s.len().max(1),
            IcuParseErrorKind::UnbalancedArmBraces,
        ))
    }
}

fn ensure_other_present(
    arms: &[(String, Vec<SubSegment>)],
    base_offset: usize,
    whole_len: usize,
) -> Result<(), IcuParseError> {
    if arms.iter().any(|(k, _)| k == "other") {
        Ok(())
    } else {
        Err(IcuParseError::at(
            base_offset,
            whole_len,
            IcuParseErrorKind::MissingOtherArm,
        ))
    }
}

/// Parses one placeholder's `inner` text (everything between the outer
/// `{`/`}`, e.g. `"count, plural, one {# item} other {# items}"`) — the
/// precondition for calling this at all is that `inner` contains at least
/// one top-level comma (`split_template`'s own trigger for treating a
/// placeholder as ICU-dispatch rather than a plain `{name}`).
pub(crate) fn parse_icu_placeholder(inner: &str) -> Result<IcuPlaceholder<'_>, IcuParseError> {
    let name_end = find_top_level_comma(inner).ok_or_else(|| {
        IcuParseError::at(0, inner.len(), IcuParseErrorKind::MissingFormatKeyword)
    })?;
    let name = inner[..name_end].trim();
    if name.is_empty() {
        return Err(IcuParseError::at(
            0,
            name_end,
            IcuParseErrorKind::EmptyPlaceholderName,
        ));
    }
    let after_name = &inner[name_end + 1..];
    let after_name_offset = name_end + 1;
    let keyword_end = find_top_level_comma(after_name);
    let (keyword_text, arms_or_style, arms_or_style_offset) = match keyword_end {
        Some(k) => (
            &after_name[..k],
            Some(&after_name[k + 1..]),
            after_name_offset + k + 1,
        ),
        None => (after_name, None, after_name_offset + after_name.len()),
    };
    let keyword = keyword_text.trim();
    let keyword_offset = after_name_offset;

    match keyword {
        "plural" | "select" => {
            let is_plural = keyword == "plural";
            let (arms_text, arms_offset) = arms_or_style
                .map(|a| (a, arms_or_style_offset))
                .ok_or_else(|| {
                    IcuParseError::at(
                        after_name_offset,
                        after_name.len(),
                        IcuParseErrorKind::MissingArmBody(String::new()),
                    )
                })?;
            let raw_arms = parse_arms(arms_text, arms_offset, is_plural, is_plural)?;
            ensure_other_present(&raw_arms, arms_offset, arms_text.len())?;
            if is_plural {
                let arms = raw_arms
                    .into_iter()
                    .map(|(k, segs)| {
                        (
                            PluralCategory::parse(&k).expect("validated by parse_arms"),
                            segs,
                        )
                    })
                    .collect();
                Ok(IcuPlaceholder {
                    name,
                    kind: PlaceholderKind::Plural { arms },
                })
            } else {
                let arms = raw_arms
                    .into_iter()
                    .map(|(k, segs)| (inner_arm_key_slice(inner, arms_offset, &k), segs))
                    .collect();
                Ok(IcuPlaceholder {
                    name,
                    kind: PlaceholderKind::Select { arms },
                })
            }
        }
        "number" => {
            let style =
                parse_optional_style(arms_or_style, arms_or_style_offset, "number", |s| match s {
                    "integer" => Some(NumberStyle::Integer),
                    "percent" => Some(NumberStyle::Percent),
                    _ => None,
                })?;
            Ok(IcuPlaceholder {
                name,
                kind: PlaceholderKind::Number { style },
            })
        }
        "date" => {
            let style =
                parse_optional_style(arms_or_style, arms_or_style_offset, "date", |s| match s {
                    "short" => Some(DateStyle::Short),
                    "medium" => Some(DateStyle::Medium),
                    "long" => Some(DateStyle::Long),
                    "full" => Some(DateStyle::Full),
                    _ => None,
                })?;
            Ok(IcuPlaceholder {
                name,
                kind: PlaceholderKind::Date { style },
            })
        }
        "selectordinal" => Err(IcuParseError::at(
            keyword_offset,
            keyword_text.len(),
            IcuParseErrorKind::UnsupportedSelectordinal,
        )),
        "" => Err(IcuParseError::at(
            after_name_offset,
            1,
            IcuParseErrorKind::MissingFormatKeyword,
        )),
        other => Err(IcuParseError::at(
            keyword_offset,
            keyword_text.len(),
            IcuParseErrorKind::UnknownFormatKeyword(other.to_string()),
        )),
    }
}

/// Re-slices `inner` at `arms_offset` to hand back a `&'a str` select-arm key
/// (select allows arbitrary keys, so unlike `PluralCategory` there's no owned
/// enum to convert to) — `k` (owned, from `parse_arms`) tells us the key's
/// text and, by construction, its byte length; re-finding it in `inner`
/// keeps `PlaceholderKind::Select`'s arm keys borrowed like `IcuPlaceholder`
/// itself, rather than introducing an asymmetric owned-`String` key only for
/// `select`.
fn inner_arm_key_slice<'a>(inner: &'a str, _arms_offset: usize, k: &str) -> &'a str {
    // `parse_arms` only ever slices arm keys directly out of the text it was
    // given (never transforms them), so `k`'s bytes are a verbatim substring
    // of `inner`; searching once for that exact substring recovers the
    // borrow. Arm keys are non-empty (checked in `parse_arms`), so this
    // always finds a match.
    let start = inner
        .find(k)
        .expect("arm key is a verbatim substring of `inner`");
    &inner[start..start + k.len()]
}

fn parse_optional_style<T>(
    arms_or_style: Option<&str>,
    arms_or_style_offset: usize,
    construct: &'static str,
    parse: impl Fn(&str) -> Option<T>,
) -> Result<Option<T>, IcuParseError> {
    let Some(s) = arms_or_style else {
        return Ok(None);
    };
    if let Some(extra_comma) = find_top_level_comma(s) {
        return Err(IcuParseError::at(
            arms_or_style_offset + extra_comma + 1,
            s.len().saturating_sub(extra_comma + 1).max(1),
            IcuParseErrorKind::TrailingContentAfterStyle(s[extra_comma + 1..].trim().to_string()),
        ));
    }
    let trimmed = s.trim();
    parse(trimmed).map(Some).ok_or_else(|| {
        IcuParseError::at(
            arms_or_style_offset,
            s.len(),
            IcuParseErrorKind::UnknownStyleKeyword {
                construct,
                found: trimmed.to_string(),
            },
        )
    })
}

/// Every ICU-dispatch placeholder in `template` — `(byte offset of `inner`
/// within `template`, `inner` text)` — for the checker's malformed-syntax
/// pass (`bynk-emit/src/project/validate.rs`). A placeholder only reaches
/// here if `split_template` already decided it was ICU-dispatch (its `inner`
/// contains a top-level comma); a plain `{name}` never does, by construction
/// (proven in `split_template`'s own doc comment).
pub(crate) fn icu_dispatch_placeholders(template: &str) -> Vec<(usize, &str)> {
    super::emit::split_template(template)
        .into_iter()
        .filter_map(|s| match s {
            super::emit::TemplateSegment::Placeholder { offset, inner } if inner.contains(',') => {
                Some((offset, inner))
            }
            _ => None,
        })
        .collect()
}

/// Every placeholder's `(name, FormatKind)` in `template`, silently dropping
/// any placeholder whose ICU parse fails — that failure is
/// `check_entry_icu_syntax`'s job to report (once), not this pure helper's,
/// so a caller comparing two locales' templates never double-reports a
/// malformed one.
pub(crate) fn template_format_kinds(template: &str) -> BTreeMap<&str, FormatKind> {
    super::emit::split_template(template)
        .into_iter()
        .filter_map(|s| match s {
            super::emit::TemplateSegment::Placeholder { inner, .. } => {
                if let Some(comma) = inner.find(',') {
                    let name = inner[..comma].trim();
                    parse_icu_placeholder(inner)
                        .ok()
                        .map(|p| (name, p.kind.format_kind()))
                } else {
                    Some((inner, FormatKind::Plain))
                }
            }
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod icu_parser_tests {
    use super::*;

    fn parse(s: &str) -> IcuPlaceholder<'_> {
        parse_icu_placeholder(s).unwrap_or_else(|e| panic!("expected Ok, got {e:?} for {s:?}"))
    }

    fn parse_err(s: &str) -> IcuParseErrorKind {
        parse_icu_placeholder(s)
            .expect_err("expected an error")
            .kind
    }

    #[test]
    fn plural_all_six_categories() {
        let p = parse(
            "n, plural, zero {none} one {#1} two {#2} few {#few} many {#many} other {#other}",
        );
        assert_eq!(p.name, "n");
        let PlaceholderKind::Plural { arms } = p.kind else {
            panic!("expected Plural")
        };
        let cats: Vec<_> = arms.iter().map(|(c, _)| *c).collect();
        assert_eq!(
            cats,
            vec![
                PluralCategory::Zero,
                PluralCategory::One,
                PluralCategory::Two,
                PluralCategory::Few,
                PluralCategory::Many,
                PluralCategory::Other,
            ]
        );
    }

    #[test]
    fn plural_hash_and_literal_segments() {
        let p = parse("n, plural, one {# item} other {# items}");
        let PlaceholderKind::Plural { arms } = p.kind else {
            panic!("expected Plural")
        };
        assert_eq!(
            arms[0].1,
            vec![SubSegment::Hash, SubSegment::Literal(" item".to_string())]
        );
        assert_eq!(
            arms[1].1,
            vec![SubSegment::Hash, SubSegment::Literal(" items".to_string())]
        );
    }

    #[test]
    fn select_arbitrary_keys() {
        let p = parse("g, select, male {He} female {She} other {They}");
        assert_eq!(p.name, "g");
        let PlaceholderKind::Select { arms } = p.kind else {
            panic!("expected Select")
        };
        assert_eq!(arms[0].0, "male");
        assert_eq!(arms[2].0, "other");
    }

    #[test]
    fn number_bare_integer_percent() {
        assert_eq!(
            parse("n, number").kind,
            PlaceholderKind::Number { style: None }
        );
        assert_eq!(
            parse("n, number, integer").kind,
            PlaceholderKind::Number {
                style: Some(NumberStyle::Integer)
            }
        );
        assert_eq!(
            parse("n, number, percent").kind,
            PlaceholderKind::Number {
                style: Some(NumberStyle::Percent)
            }
        );
    }

    #[test]
    fn date_bare_and_all_four_styles() {
        assert_eq!(parse("d, date").kind, PlaceholderKind::Date { style: None });
        for (kw, expect) in [
            ("short", DateStyle::Short),
            ("medium", DateStyle::Medium),
            ("long", DateStyle::Long),
            ("full", DateStyle::Full),
        ] {
            assert_eq!(
                parse(&format!("d, date, {kw}")).kind,
                PlaceholderKind::Date {
                    style: Some(expect)
                }
            );
        }
    }

    #[test]
    fn quoting_doubled_quote_is_literal_apostrophe() {
        let p = parse("n, plural, one {it''s one} other {it''s other}");
        let PlaceholderKind::Plural { arms } = p.kind else {
            panic!("expected Plural")
        };
        assert_eq!(arms[0].1, vec![SubSegment::Literal("it's one".to_string())]);
    }

    #[test]
    fn quoting_escapes_literal_brace() {
        let p = parse("n, plural, one {'{'} other {ok}");
        let PlaceholderKind::Plural { arms } = p.kind else {
            panic!("expected Plural")
        };
        assert_eq!(arms[0].1, vec![SubSegment::Literal("{".to_string())]);
    }

    #[test]
    fn quoting_escapes_literal_hash() {
        let p = parse("n, plural, one {'#'} other {ok}");
        let PlaceholderKind::Plural { arms } = p.kind else {
            panic!("expected Plural")
        };
        assert_eq!(arms[0].1, vec![SubSegment::Literal("#".to_string())]);
    }

    #[test]
    fn multibyte_arm_body() {
        let p = parse("n, select, other {caf\u{e9} \u{1f980}}");
        let PlaceholderKind::Select { arms } = p.kind else {
            panic!("expected Select")
        };
        assert_eq!(
            arms[0].1,
            vec![SubSegment::Literal("caf\u{e9} \u{1f980}".to_string())]
        );
    }

    #[test]
    fn err_empty_name() {
        assert_eq!(
            parse_err(", plural, other {x}"),
            IcuParseErrorKind::EmptyPlaceholderName
        );
    }

    #[test]
    fn err_missing_format_keyword_no_second_comma() {
        assert_eq!(parse_err("n"), IcuParseErrorKind::MissingFormatKeyword);
    }

    #[test]
    fn err_unknown_format_keyword() {
        assert_eq!(
            parse_err("d, duration"),
            IcuParseErrorKind::UnknownFormatKeyword("duration".to_string())
        );
    }

    #[test]
    fn err_selectordinal_unsupported() {
        assert_eq!(
            parse_err("rank, selectordinal, one {#st} two {#nd} few {#rd} other {#th}"),
            IcuParseErrorKind::UnsupportedSelectordinal
        );
    }

    #[test]
    fn err_plural_offset_unsupported() {
        assert_eq!(
            parse_err("n, plural, offset:1 one {#} other {#}"),
            IcuParseErrorKind::UnsupportedPluralOffset
        );
    }

    #[test]
    fn err_plural_exact_value_arm_unsupported() {
        assert_eq!(
            parse_err("n, plural, =0 {none} one {#} other {#}"),
            IcuParseErrorKind::UnsupportedPluralExactValueArm("=0".to_string())
        );
    }

    #[test]
    fn err_unknown_plural_category() {
        assert_eq!(
            parse_err("n, plural, teen {x} other {y}"),
            IcuParseErrorKind::UnknownPluralCategory("teen".to_string())
        );
    }

    #[test]
    fn err_duplicate_plural_arm() {
        // PR #879 review (finding 1): a repeated category compiled fine
        // before this fix and emitted a duplicate-property object literal
        // `tsc --strict` rejects (TS1117).
        assert_eq!(
            parse_err("n, plural, one {a} one {b} other {c}"),
            IcuParseErrorKind::DuplicateArmKey("one".to_string())
        );
    }

    #[test]
    fn err_duplicate_select_arm() {
        assert_eq!(
            parse_err("g, select, male {a} male {b} other {c}"),
            IcuParseErrorKind::DuplicateArmKey("male".to_string())
        );
    }

    #[test]
    fn err_missing_other_arm() {
        assert_eq!(
            parse_err("n, plural, one {# item}"),
            IcuParseErrorKind::MissingOtherArm
        );
    }

    #[test]
    fn err_hash_outside_plural_arm() {
        assert_eq!(
            parse_err("g, select, male {# things} female {things} other {things}"),
            IcuParseErrorKind::HashOutsidePluralArm
        );
    }

    #[test]
    fn err_nested_dispatch_in_sub_message() {
        assert_eq!(
            parse_err("n, plural, one {nested {m, number}} other {ok}"),
            IcuParseErrorKind::NestedDispatchInSubMessage
        );
    }

    #[test]
    fn err_unbalanced_arm_braces() {
        // The `one` arm's own `{` is never closed at all — distinct from
        // `err_nested_dispatch_in_sub_message`'s case, where a *second*
        // unescaped `{` appears before any close.
        assert_eq!(
            parse_err("n, plural, one {# item"),
            IcuParseErrorKind::UnbalancedArmBraces
        );
    }

    #[test]
    fn err_unknown_number_style() {
        assert_eq!(
            parse_err("n, number, currency"),
            IcuParseErrorKind::UnknownStyleKeyword {
                construct: "number",
                found: "currency".to_string(),
            }
        );
    }

    #[test]
    fn err_unknown_date_style() {
        assert_eq!(
            parse_err("d, date, yyyy"),
            IcuParseErrorKind::UnknownStyleKeyword {
                construct: "date",
                found: "yyyy".to_string(),
            }
        );
    }

    #[test]
    fn err_trailing_content_after_style() {
        assert_eq!(
            parse_err("n, number, integer, extra"),
            IcuParseErrorKind::TrailingContentAfterStyle("extra".to_string())
        );
    }

    #[test]
    fn find_icu_close_handles_nested_arms_and_quoting() {
        let rest = "count, plural, one {'{'} other {#}} tail";
        let close = find_icu_close(rest).expect("should close");
        assert_eq!(&rest[..close], "count, plural, one {'{'} other {#}");
    }

    #[test]
    fn template_format_kinds_mixed() {
        let kinds = template_format_kinds(
            "{a} {b, plural, one {#} other {#}} {c, select, x {x} other {o}} {d, number} {e, date}",
        );
        assert_eq!(kinds.get("a"), Some(&FormatKind::Plain));
        assert_eq!(kinds.get("b"), Some(&FormatKind::Plural));
        assert_eq!(kinds.get("c"), Some(&FormatKind::Select));
        assert_eq!(kinds.get("d"), Some(&FormatKind::Number));
        assert_eq!(kinds.get("e"), Some(&FormatKind::Date));
    }

    #[test]
    fn icu_dispatch_placeholders_skips_plain_ones() {
        let found = icu_dispatch_placeholders("hi {name}, you have {n, plural, one {#} other {#}}");
        assert_eq!(found.len(), 1);
        assert!(found[0].1.starts_with("n, plural"));
    }
}
