//! v0.17: completion for the adapter/capability surface.
//!
//! Three cursor contexts are recognised from the line up to the cursor:
//!
//! - after `consumes ` — consumable units (contexts, adapters, and the `karn`
//!   surface);
//! - inside `consumes U { … }` — the capabilities `U` exports;
//! - after `given …` — the capabilities in scope (local, flattened via a braced
//!   `consumes`, and qualified `U.Cap` for whole-unit `consumes`).
//!
//! Unit/capability enumeration parses the project's `.karn` files (and the
//! embedded `karn` surface) with recovery, so it works even while the file the
//! cursor sits in is mid-edit.

use std::collections::BTreeSet;
use std::path::Path;

use karnc::ast::{ExportKind, SourceUnit};
use karnc::firstparty::KARN_ADAPTER_SRC;
use karnc::{lexer, parser};

use crate::symbols::walk_karn_files;

/// What a candidate refers to — maps to an LSP `CompletionItemKind`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Unit,
    Capability,
}

pub struct Completion {
    pub label: String,
    pub kind: CompletionKind,
    pub detail: Option<String>,
}

/// Produce completions for the cursor, given the text of the line up to the
/// cursor, the current document text, and the project source root (if any).
pub fn complete(line_prefix: &str, doc_text: &str, src_root: Option<&Path>) -> Vec<Completion> {
    // 1. Inside `consumes U { … <cursor>` — the capabilities U exports.
    if let Some(unit) = consumes_brace_unit(line_prefix) {
        return capabilities_of_unit(&unit, doc_text, src_root)
            .into_iter()
            .map(|c| Completion {
                label: c,
                kind: CompletionKind::Capability,
                detail: Some(format!("capability exported by `{unit}`")),
            })
            .collect();
    }
    // 2. After `consumes <prefix>` — consumable unit names.
    if is_consumes_target(line_prefix) {
        return consumable_units(doc_text, src_root);
    }
    // 3. After `given …` — in-scope capabilities.
    if is_given_position(line_prefix) {
        return in_scope_capabilities(doc_text, src_root);
    }
    Vec::new()
}

// -- Cursor-context detection (line-prefix scanning) --

/// `consumes U { … ` with the brace still open at the cursor → `Some(U)`.
fn consumes_brace_unit(line: &str) -> Option<String> {
    let idx = line.rfind("consumes")?;
    let after = &line[idx + "consumes".len()..];
    let open = after.find('{')?;
    // The brace must still be open up to the cursor (no closing brace after it).
    if after[open + 1..].contains('}') {
        return None;
    }
    let unit = after[..open].trim();
    if unit.is_empty() || !is_qualified_name(unit) {
        return None;
    }
    Some(unit.to_string())
}

/// `consumes <partial>` with no brace or `as` yet → completing the target name.
fn is_consumes_target(line: &str) -> bool {
    let Some(idx) = line.rfind("consumes") else {
        return false;
    };
    // `consumes` must be a standalone keyword (preceded by start/whitespace).
    if !line[..idx]
        .chars()
        .last()
        .map(|c| c.is_whitespace())
        .unwrap_or(true)
    {
        return false;
    }
    let after = &line[idx + "consumes".len()..];
    // Need at least one separating space, and no `{`, `}`, or `as` yet.
    after.starts_with(char::is_whitespace)
        && !after.contains('{')
        && !after.contains('}')
        && !after.split_whitespace().any(|w| w == "as")
}

/// The cursor is inside a `given` list (after `given`, before the `{` body).
fn is_given_position(line: &str) -> bool {
    let Some(idx) = line.rfind("given") else {
        return false;
    };
    if !line[..idx]
        .chars()
        .last()
        .map(|c| c.is_whitespace())
        .unwrap_or(true)
    {
        return false;
    }
    let after = &line[idx + "given".len()..];
    if !after.starts_with(char::is_whitespace) {
        return false;
    }
    // Still in the given list while only capability names, dots, commas and
    // whitespace follow — a `{` opens the handler body.
    after
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | ',' | ' ' | '\t'))
}

fn is_qualified_name(s: &str) -> bool {
    !s.is_empty()
        && s.split('.').all(|seg| {
            !seg.is_empty()
                && seg.chars().all(|c| c.is_alphanumeric() || c == '_')
                && !seg.chars().next().unwrap().is_ascii_digit()
        })
}

// -- Enumeration (parse project sources + the embedded `karn` surface) --

/// Parse every project unit, plus the embedded `karn` surface, and call `f` for
/// each. Recovery parsing tolerates the in-progress edit at the cursor.
fn for_each_unit(doc_text: &str, src_root: Option<&Path>, mut f: impl FnMut(&SourceUnit)) {
    let mut sources: Vec<String> = vec![KARN_ADAPTER_SRC.to_string(), doc_text.to_string()];
    if let Some(root) = src_root {
        for path in walk_karn_files(root) {
            if let Ok(s) = std::fs::read_to_string(&path) {
                sources.push(s);
            }
        }
    }
    for src in &sources {
        let Ok(tokens) = lexer::tokenize(src) else {
            continue;
        };
        let (unit, _errs) = parser::parse_unit_with_recovery(&tokens, src);
        if let Some(unit) = unit {
            f(&unit);
        }
    }
}

/// Consumable unit names: contexts and adapters (plus `karn`), deduplicated.
fn consumable_units(doc_text: &str, src_root: Option<&Path>) -> Vec<Completion> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut out: Vec<Completion> = Vec::new();
    for_each_unit(doc_text, src_root, |unit| {
        let (name, kind) = match unit {
            SourceUnit::Context(c) => (c.name.joined(), "context"),
            SourceUnit::Adapter(a) => (a.name.joined(), "adapter"),
            _ => return,
        };
        if seen.insert(name.clone()) {
            out.push(Completion {
                label: name,
                kind: CompletionKind::Unit,
                detail: Some(kind.to_string()),
            });
        }
    });
    out
}

/// The capability names a unit `exports capability`.
fn capabilities_of_unit(unit: &str, doc_text: &str, src_root: Option<&Path>) -> Vec<String> {
    let mut out: BTreeSet<String> = BTreeSet::new();
    for_each_unit(doc_text, src_root, |u| {
        let (name, exports) = match u {
            SourceUnit::Context(c) => (c.name.joined(), &c.exports),
            SourceUnit::Adapter(a) => (a.name.joined(), &a.exports),
            _ => return,
        };
        if name != unit {
            return;
        }
        for clause in exports {
            if clause.kind == ExportKind::Capability {
                for n in &clause.names {
                    out.insert(n.name.clone());
                }
            }
        }
    });
    out.into_iter().collect()
}

/// Capabilities in scope for a `given` clause in the current document: locally
/// declared capabilities, bare names flattened by a braced `consumes`, and
/// `U.Cap` for each whole-unit `consumes U`.
fn in_scope_capabilities(doc_text: &str, src_root: Option<&Path>) -> Vec<Completion> {
    let mut labels: BTreeSet<String> = BTreeSet::new();
    let Ok(tokens) = lexer::tokenize(doc_text) else {
        return Vec::new();
    };
    let (Some(unit), _errs) = parser::parse_unit_with_recovery(&tokens, doc_text) else {
        return Vec::new();
    };
    let (items, consumes) = match &unit {
        SourceUnit::Context(c) => (&c.items, &c.consumes),
        SourceUnit::Adapter(a) => (&a.items, &EMPTY_CONSUMES),
        _ => return Vec::new(),
    };
    // Locally declared capabilities.
    for item in items {
        if let karnc::ast::CommonsItem::Capability(c) = item {
            labels.insert(c.name.name.clone());
        }
    }
    // Consumed capabilities: flattened bare names, or qualified `U.Cap`.
    for c in consumes {
        let unit_name = c.target.joined();
        match &c.selected {
            Some(names) => {
                for n in names {
                    labels.insert(n.name.clone());
                }
            }
            None => {
                let prefix = c
                    .alias
                    .as_ref()
                    .map(|a| a.name.clone())
                    .unwrap_or_else(|| unit_name.clone());
                for cap in capabilities_of_unit(&unit_name, doc_text, src_root) {
                    labels.insert(format!("{prefix}.{cap}"));
                }
            }
        }
    }
    labels
        .into_iter()
        .map(|label| Completion {
            label,
            kind: CompletionKind::Capability,
            detail: Some("capability in scope".to_string()),
        })
        .collect()
}

static EMPTY_CONSUMES: Vec<karnc::ast::ConsumesDecl> = Vec::new();

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(line: &str, doc: &str) -> Vec<String> {
        complete(line, doc, None)
            .into_iter()
            .map(|c| c.label)
            .collect()
    }

    #[test]
    fn consumes_target_suggests_units_including_karn() {
        // An adapter in the open doc plus the always-available `karn` surface.
        let doc = "adapter tokens {\n  binding \"./b.ts\"\n  capability Jwt { fn f() -> Effect[Int] }\n  provides Jwt = X\n}\n";
        let got = labels("  consumes ", doc);
        assert!(got.contains(&"karn".to_string()), "{got:?}");
        assert!(got.contains(&"tokens".to_string()), "{got:?}");
    }

    #[test]
    fn consumes_brace_suggests_that_units_capabilities() {
        let got = labels("  consumes karn { ", "context a.b\n");
        // The embedded `karn` surface exports these.
        assert!(got.contains(&"Clock".to_string()), "{got:?}");
        assert!(got.contains(&"Random".to_string()), "{got:?}");
        assert!(got.contains(&"Logger".to_string()), "{got:?}");
    }

    #[test]
    fn given_suggests_local_and_flattened_capabilities() {
        let doc = "context a.b\n\
                   consumes karn { Clock }\n\
                   capability Local { fn f() -> Effect[Int] }\n\
                   service s {\n\
                   on call() -> Effect[Int] given Clock {\n\
                   1\n\
                   }\n\
                   }\n";
        let got = labels("    on call() -> Effect[Int] given ", doc);
        assert!(got.contains(&"Clock".to_string()), "flattened: {got:?}");
        assert!(got.contains(&"Local".to_string()), "local: {got:?}");
    }

    #[test]
    fn no_completion_in_plain_position() {
        assert!(labels("  let x = ", "context a.b\n").is_empty());
    }

    #[test]
    fn consumes_with_as_is_not_a_target_completion() {
        // `consumes X as ` is aliasing, not target-name completion.
        assert!(!is_consumes_target("consumes platform.time as "));
        assert!(is_consumes_target("consumes platform"));
    }
}
