//! The textual lint over [`crate::TsStmt::verbatim`] content (Q2, `design/
//! tracks/the-typescript-tree.md` §3.2's "real gap this settling pass found
//! and closes"). A byte-golden fixture is blind to what's *inside* an
//! opaque `Verbatim` block — this scans the wrapped text directly for the
//! six constructs R7.1 forbids the tree from ever representing (`enum`,
//! `namespace`, a decorator, a constructor parameter property, `: any`/
//! `as any`), so a `Verbatim` block smuggling one of them in stays visible
//! even while every golden fixture stays green.
//!
//! Pattern match over text, not a real TS parser — same posture `xtask`'s
//! own `ts_any` probe (`xtask/src/greenfield_status.rs`) already takes for
//! the identical `any` patterns, reused here rather than re-derived.
//!
//! `bynk-emit` builds no `Verbatim` content in this slice (#1307's Decision
//! C), so nothing calls this over real output yet — it exists, tested
//! against one positive and one negative case per construct, ready for Arc C
//! (Decision F: wiring it into a real CI-visible check over compiled output
//! is meaningful only once real `Verbatim` content exists to check).

/// One construct [`verbatim_violations`] found, and the line it was on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub construct: &'static str,
    pub line: String,
}

/// Scan `text` (a `Verbatim` statement's own wrapped TypeScript) for every
/// line matching one of the six banned constructs. Order of the checks
/// within a line matters only for which `construct` label a line already
/// matching two patterns gets — real emitted lines don't do that in
/// practice, so the first match wins and the rest of that line isn't
/// checked further.
pub fn verbatim_violations(text: &str) -> Vec<Violation> {
    let mut out = Vec::new();
    for line in text.lines() {
        if let Some(construct) = detect(line) {
            out.push(Violation {
                construct,
                line: line.to_string(),
            });
        }
    }
    out
}

fn detect(line: &str) -> Option<&'static str> {
    if is_any(line) {
        return Some("TsType::Any");
    }
    if contains_keyword(line, "enum") {
        return Some("enum");
    }
    if contains_keyword(line, "namespace") {
        return Some("namespace");
    }
    if is_decorator(line) {
        return Some("decorator");
    }
    if is_constructor_parameter_property(line) {
        return Some("constructor parameter property");
    }
    None
}

/// The same five shapes `xtask`'s own `ts_any` probe (`line_violates_ts_any`)
/// checks against Rust source — `as any`, bare `: any`, generic-position
/// `<any`/`any>`/`any[]` — but *not* that probe's own plain-substring
/// matching (review of #1308, finding 4): that probe scans Bynk's own Rust
/// source, a corpus the team can rename around a false positive; this scans
/// generated TypeScript carrying arbitrary Bynk-author identifiers nobody
/// here controls (`Company[]`, `Record<string, Company>`, `Map<anything,
/// …>` all contain one of the five substrings and none is `Any`), so a
/// false positive here is a user-facing build failure with no user-side
/// fix. `any` is matched as its own word first (reusing [`contains_keyword`]'s
/// boundary rule), then classified by what's immediately around it.
fn is_any(line: &str) -> bool {
    let bytes = line.as_bytes();
    let mut start = 0;
    while let Some(rel) = line[start..].find("any") {
        let i = start + rel;
        let before_ok = i == 0 || !is_ident_byte(bytes[i - 1]);
        let after_ok = i + 3 >= bytes.len() || !is_ident_byte(bytes[i + 3]);
        if before_ok && after_ok && is_any_type_position(&line[..i], &line[i + 3..]) {
            return true;
        }
        start = i + 3;
    }
    false
}

/// Whether a word-bounded `any` sits in a type position: `as any`, `: any`
/// (with or without the space — `const x:any` is real, emitted-output-
/// unlikely but still a live pattern to catch), a generic open (`<any`), a
/// generic close (`any>`), or an array (`any[]`). `before`/`after` are the
/// line's text on each side of the matched word.
fn is_any_type_position(before: &str, after: &str) -> bool {
    let trimmed = before.trim_end();
    if trimmed.ends_with("as") {
        let as_start_ok =
            trimmed.len() == 2 || !is_ident_byte(trimmed.as_bytes()[trimmed.len() - 3]);
        if as_start_ok {
            return true;
        }
    }
    if trimmed.ends_with(':') || trimmed.ends_with('<') {
        return true;
    }
    after.starts_with('>') || after.starts_with("[]")
}

/// Whether `keyword` appears in `line` as a real word — not as a substring
/// of a longer identifier (`enum` inside `enumerate`, `namespace` inside
/// `MyNamespaceThing`).
fn contains_keyword(line: &str, keyword: &str) -> bool {
    let bytes = line.as_bytes();
    let klen = keyword.len();
    let mut start = 0;
    while let Some(rel) = line[start..].find(keyword) {
        let i = start + rel;
        let before_ok = i == 0 || !is_ident_byte(bytes[i - 1]);
        let after_ok = i + klen >= bytes.len() || !is_ident_byte(bytes[i + klen]);
        if before_ok && after_ok {
            return true;
        }
        start = i + klen;
    }
    false
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

/// A TypeScript decorator: `@Identifier` at the start of a (trimmed) line —
/// `@Injectable()`, `@Component({ ... })`. Emitted output never has a
/// legitimate `@` at line-start otherwise (no JSDoc `@param` lines survive
/// into `Verbatim` text; those live in comments this scan doesn't need to
/// special-case since a `@param` line's next character is a space, not an
/// identifier start).
fn is_decorator(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed
        .strip_prefix('@')
        .and_then(|rest| rest.chars().next())
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
}

/// A constructor parameter property: `private`/`public`/`protected`/
/// `readonly` inside a `constructor(...)`'s own parameter list — the one
/// type-directed construct pure strip-only stripping cannot erase (ADR
/// 0136's own strip-only rationale, already the reason `emitter/emit.rs`'s
/// own provider constructor de-sugars away from this shape by hand). Scoped
/// to *only* the text between `constructor(` and its matching close paren
/// (review of #1308, finding 5: the first version scanned to end of line,
/// so `constructor(deps: Deps) { this.mode = "readonly access"; }` — a
/// `readonly`-shaped *string literal* in the constructor's own body —
/// false-positived, contradicting this doc comment's own claim).
/// Paren-depth tracked, not `{}`-depth: a parameter's own object type
/// (`constructor(deps: { Log: unknown })`) carries braces the scan must
/// walk straight through, so only `(`/`)` count.
fn is_constructor_parameter_property(line: &str) -> bool {
    let Some(after) = line
        .find("constructor(")
        .map(|i| &line[i + "constructor(".len()..])
    else {
        return false;
    };
    let mut depth = 1i32;
    let mut end = after.len();
    for (idx, c) in after.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    end = idx;
                    break;
                }
            }
            _ => {}
        }
    }
    let params = &after[..end];
    ["private ", "public ", "protected ", "readonly "]
        .iter()
        .any(|kw| params.contains(kw))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catches_as_any_and_bare_colon_any() {
        assert_eq!(
            verbatim_violations("const x = (value as any).field;"),
            vec![Violation {
                construct: "TsType::Any",
                line: "const x = (value as any).field;".to_string(),
            }]
        );
        assert!(verbatim_violations("const x: unknown = value;").is_empty());
    }

    #[test]
    fn catches_generic_position_any() {
        assert!(!verbatim_violations("const h: Record<string, any[]> = {};").is_empty());
        assert!(!verbatim_violations("type T = Array<any>;").is_empty());
    }

    #[test]
    fn catches_colon_any_with_no_space() {
        assert!(!verbatim_violations("function f(x:any) {}").is_empty());
    }

    /// Review of #1308, finding 4: `is_any`'s original plain-substring match
    /// flagged `any[]`/`any>`/`<any` wherever they appeared, including
    /// inside an unrelated identifier — a real hazard here specifically,
    /// since this scans generated TypeScript carrying Bynk-author schema
    /// names nobody on this team can rename to dodge a false positive.
    #[test]
    fn does_not_false_positive_on_any_as_a_substring_of_a_real_identifier() {
        assert!(verbatim_violations("type Fleet = Company[];").is_empty());
        assert!(verbatim_violations("const x: Record<string, Company> = {};").is_empty());
        assert!(verbatim_violations("type T = Map<anything, string>;").is_empty());
    }

    #[test]
    fn catches_enum_as_a_real_keyword_not_a_substring() {
        assert_eq!(
            verbatim_violations("enum Colour { Red, Green }")[0].construct,
            "enum"
        );
        assert!(verbatim_violations("function enumerate(x: string) {}").is_empty());
        assert!(verbatim_violations("const myEnum = 1;").is_empty());
    }

    #[test]
    fn catches_namespace_as_a_real_keyword_not_a_substring() {
        assert_eq!(
            verbatim_violations("namespace Foo { export const x = 1; }")[0].construct,
            "namespace"
        );
        assert!(verbatim_violations("const namespaced = true;").is_empty());
    }

    #[test]
    fn catches_a_leading_decorator() {
        assert_eq!(
            verbatim_violations("  @Injectable()")[0].construct,
            "decorator"
        );
        assert_eq!(
            verbatim_violations("@Component({ selector: \"x\" })")[0].construct,
            "decorator"
        );
        // A bare `@` with no following identifier (an email-shaped string
        // literal fragment, say) isn't a decorator.
        assert!(verbatim_violations("const s = \"a@b.com\";").is_empty());
    }

    #[test]
    fn catches_constructor_parameter_properties() {
        assert_eq!(
            verbatim_violations("constructor(private deps: Deps) {}")[0].construct,
            "constructor parameter property"
        );
        assert_eq!(
            verbatim_violations("constructor(a: A, readonly b: B) {}")[0].construct,
            "constructor parameter property"
        );
        // The de-sugared shape `bynk-emit` actually emits — a plain param,
        // assigned in the body — is not a parameter property.
        assert!(
            verbatim_violations("constructor(deps: { Log: unknown }) { this.deps = deps; }")
                .is_empty()
        );
    }

    /// Review of #1308, finding 5: the original scan ran to end of line, so
    /// a `readonly`-shaped string *inside the constructor's own body* (not
    /// its parameter list) false-positived — contradicting the function's
    /// own doc comment, which already claimed the scan was parameter-list-
    /// scoped.
    #[test]
    fn does_not_false_positive_on_the_constructor_body() {
        assert!(
            verbatim_violations("constructor(deps: Deps) { this.mode = \"readonly access\"; }")
                .is_empty()
        );
    }

    #[test]
    fn clean_typescript_produces_no_violations() {
        let text = "export function add(a: number, b: number): number {\n  return a + b;\n}\n";
        assert!(verbatim_violations(text).is_empty());
    }

    #[test]
    fn scans_every_offending_line_not_just_the_first() {
        let text = "enum A { X }\nconst y: any = 1;\n";
        let violations = verbatim_violations(text);
        assert_eq!(violations.len(), 2);
        assert_eq!(violations[0].construct, "enum");
        assert_eq!(violations[1].construct, "TsType::Any");
    }
}
