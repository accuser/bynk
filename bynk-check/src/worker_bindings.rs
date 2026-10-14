//! A Worker's binding names must be unique (`bynk.context.duplicate_binding`).
//!
//! A Worker's bindings come from derivations that know nothing of each other:
//! the KV namespace's fixed [`crate::firstparty::KV_BINDING_NAME`], a consumed
//! context's name (`a.b` → `A_B`) and an agent's class name (`OrderEntity` →
//! `ORDER_ENTITY`). Two of them can meet: an agent `Kv` in a context that
//! consumes `bynk.cloudflare { Kv }` is `KV` twice. Wrangler takes such a
//! config without complaint and one binding hides the other, so the clash is
//! a compile error. The names themselves are never adjusted to avoid it,
//! because deployed Workers depend on them.
//!
//! The names are the emitter's (`bynk_emit::emitter::wrangler::
//! worker_bindings`, the list `wrangler.toml` is written from); this module
//! owns the check over them and the diagnostic.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use bynk_project::ParsedFile;
use bynk_syntax::ast::{CommonsItem, SourceUnit};
use bynk_syntax::error::CompileError;
use bynk_syntax::span::Span;

use crate::firstparty;
use crate::project_model::ErrorSink;

/// What put a binding into a Worker's `wrangler.toml`: the source a
/// `bynk.context.duplicate_binding` diagnostic names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindingSource {
    /// A `[[services]]` Service Binding to another context's Worker: one the
    /// context consumes, or one subscribing to an event it publishes.
    Service { context: String },
    /// The `[[kv_namespaces]]` binding the cloudflare adapter's `Kv` reads.
    Kv,
    /// A `[[durable_objects.bindings]]` entry for one of the context's agents.
    Agent { class_name: String },
    /// The `[[durable_objects.bindings]]` entry for the events fan-out class
    /// of a context whose handlers emit.
    EventsFanout { class_name: String },
}

impl BindingSource {
    /// The source in a diagnostic's words.
    pub fn describe(&self) -> String {
        match self {
            Self::Service { context } => format!("the Service Binding to context `{context}`"),
            Self::Kv => format!(
                "the KV namespace binding of `{}`'s `Kv`",
                firstparty::CLOUDFLARE_UNIT
            ),
            Self::Agent { class_name } => {
                format!("the Durable Object binding of agent `{class_name}`")
            }
            Self::EventsFanout { .. } => {
                "the Durable Object binding of the events fan-out".to_string()
            }
        }
    }
}

/// One binding a Worker declares: the name its `env` field and its
/// `wrangler.toml` entry share, and where that name came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerBinding {
    pub name: String,
    pub source: BindingSource,
}

/// The bindings in `bindings` whose name an earlier one already took, each
/// paired with that earlier one, in list order.
pub fn duplicate_bindings(bindings: &[WorkerBinding]) -> Vec<(&WorkerBinding, &WorkerBinding)> {
    let mut first: HashMap<&str, &WorkerBinding> = HashMap::new();
    let mut duplicates = Vec::new();
    for binding in bindings {
        match first.get(binding.name.as_str()) {
            Some(earlier) => duplicates.push((*earlier, binding)),
            None => {
                first.insert(&binding.name, binding);
            }
        }
    }
    duplicates
}

/// `bynk.context.duplicate_binding`: each context's Worker declares every
/// binding name once. `bindings` holds each context's bindings as its
/// `wrangler.toml` would list them. One error per clash, at the declaration
/// that brought in the second binding (an agent, or a `consumes` clause),
/// preferring one the author wrote over `consumes bynk.cloudflare`, naming
/// both sources. A clash between two sources with no declaration of their own
/// (Service Bindings added for event subscribers) is reported at the
/// context's name.
pub fn phase_duplicate_bindings(
    parsed: &[ParsedFile],
    groups: &BTreeMap<String, Vec<usize>>,
    bindings: &BTreeMap<String, Vec<WorkerBinding>>,
    errors: &mut ErrorSink,
) {
    for (ctx, ctx_bindings) in bindings {
        let indices = groups.get(ctx).map(Vec::as_slice).unwrap_or_default();
        for (earlier, later) in duplicate_bindings(ctx_bindings) {
            // At the declaration the author can change: the later source's,
            // else the earlier's, but never the `bynk.cloudflare` clause
            // while the other source has a declaration of its own.
            let mut sources = [later, earlier];
            sources.sort_by_key(|b| b.source == BindingSource::Kv);
            let (file, span) = sources
                .iter()
                .find_map(|b| declared_at(parsed, indices, b))
                .or_else(|| context_header(parsed, indices))
                .map_or((None, Span::default()), |(file, span)| (Some(file), span));
            errors.push_for(
                file.as_deref(),
                CompileError::new(
                    "bynk.context.duplicate_binding",
                    span,
                    format!(
                        "the Worker for `{ctx}` would declare two bindings named `{}`: {} and {}",
                        later.name,
                        earlier.source.describe(),
                        later.source.describe(),
                    ),
                )
                .with_note(
                    "a Worker's bindings share one `env`, so each name must be unique; binding names follow agent and context names, so rename the agent or context behind one of them",
                ),
            );
        }
    }
}

/// Where the context declares the source of `binding`: an agent's name, or
/// the `consumes` clause naming a context (or `bynk.cloudflare`, for the KV
/// namespace). `None` for a source with no declaration of its own: the events
/// fan-out, or a Service Binding added for an event subscriber.
fn declared_at(
    parsed: &[ParsedFile],
    indices: &[usize],
    binding: &WorkerBinding,
) -> Option<(PathBuf, Span)> {
    indices.iter().find_map(|&i| {
        let SourceUnit::Context(c) = parsed[i].unit() else {
            return None;
        };
        let consumed = |target: &str| {
            c.consumes
                .iter()
                .find(|d| d.target.joined() == target)
                .map(|d| d.span)
        };
        let span = match &binding.source {
            BindingSource::Agent { class_name } => c.items.iter().find_map(|item| {
                let CommonsItem::Agent(a) = item else {
                    return None;
                };
                (a.name.name == *class_name).then_some(a.name.span)
            }),
            BindingSource::Service { context } => consumed(context),
            BindingSource::Kv => consumed(firstparty::CLOUDFLARE_UNIT),
            BindingSource::EventsFanout { .. } => None,
        }?;
        Some((parsed[i].identity_path(), span))
    })
}

/// The context's name in its first file: where a clash between two sources
/// with no declaration of their own is reported.
fn context_header(parsed: &[ParsedFile], indices: &[usize]) -> Option<(PathBuf, Span)> {
    indices.iter().find_map(|&i| {
        let SourceUnit::Context(c) = parsed[i].unit() else {
            return None;
        };
        Some((parsed[i].identity_path(), c.name.span))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service(name: &str, context: &str) -> WorkerBinding {
        WorkerBinding {
            name: name.to_string(),
            source: BindingSource::Service {
                context: context.to_string(),
            },
        }
    }

    #[test]
    fn distinct_names_do_not_clash() {
        assert!(duplicate_bindings(&[service("A", "a"), service("B", "b")]).is_empty());
    }

    /// Three bindings on one name are two clashes, each against the first.
    #[test]
    fn every_repeat_is_reported_against_the_first() {
        let bindings = [
            service("A_B", "a.b"),
            service("A_B", "a_b"),
            service("A_B", "x"),
        ];
        let duplicates = duplicate_bindings(&bindings);
        assert_eq!(duplicates.len(), 2);
        assert!(
            duplicates
                .iter()
                .all(|(earlier, _)| *earlier == &bindings[0])
        );
        assert_eq!(duplicates[1].1, &bindings[2]);
    }
}
