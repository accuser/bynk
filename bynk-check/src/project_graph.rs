//! P8.3 (#1514): a typed `ProjectGraph` — R3.13's project-level query, and
//! the deferral ADR 0326 opened at phase 4 ("that full shape is phase 8's,
//! gated on phase 8 opening in turn") and ADR 0388 both named as landing
//! here. **Data-model half only** (`design/tracks/incrementality.md` §6):
//! this module builds and populates the type from resolved facts
//! `project_model.rs` already computes; it does not migrate `graph.rs`'s
//! existing name-keyed cycle detection or `bynk-emit`'s compose-root
//! generator (R8.16) onto it (\[DECISION D\]).
//!
//! See `design/pending/p8-3-project-graph.md`'s own ADR block (number
//! assigned at merge) for the full reasoning behind every decision below,
//! including [DECISION E] — why this type lives in `bynk-check`, not
//! `bynk-project` as this slice's own issue first proposed.
//!
//! - [DECISION A]: the reference's own `contract: IndexVec<UnitId,
//!   ContractHash>` field is deferred entirely — no `ContractHash` type
//!   exists anywhere in this tree, and building one is real, unscoped work
//!   with no named consumer yet.
//! - [DECISION B]/[DECISION E]: `units`/`files` are plain `HashMap`s, not an
//!   `IndexVec` (no `index_vec` crate in this workspace, same posture P8.1's
//!   own Decision A took) and not a `Vec` indexed by `UnitId`'s own bytes
//!   either — `UnitId` (P8.1, #1512) is a `String` newtype, not a dense
//!   integer, so a byte-indexed `Vec` was never available to begin with.
//!   This resolves the fork P8.1's own Decision A explicitly left open
//!   ("whether `ProjectGraph` adapts to a string-keyed `UnitId`"): it does.
//! - [DECISION C]: `EdgeKind::Provides` is built as the structural dual of
//!   `EdgeKind::Consumes` (whenever A consumes from B, B provides to A) —
//!   not a separately-resolved fact, because no such fact exists in the
//!   tree today (`phase_validate_providers` diagnoses per-unit provider/
//!   capability consistency, not inter-unit edges). Building the dual from
//!   `unit_consumes` alone, the one resolved map this slice already reads,
//!   avoids inventing new resolution logic no other consumer needs yet.
//! - [DECISION E]: this type and its builder live in `bynk-check`, not
//!   `bynk-project` as this slice's own issue first proposed. Two real
//!   blockers, both confirmed against the live `Cargo.toml`s and source, not
//!   assumed from the issue's own framing: `bynk-project` cannot depend on
//!   `bynk-check` (the crate graph runs the other way — `bynk-check`
//!   already depends on `bynk-project`), so it could never reach P8.1's
//!   `UnitId`; and the `uses`/`consumes` edges this builder needs are
//!   resolved facts computed by `bynk-check::project_model`'s own
//!   `phase_resolve_uses`/`phase_resolve_consumes` — `bynk-project`'s own
//!   `discovery.rs` only parses files, it never resolves a cross-unit
//!   reference. Moving either `UnitId` or the resolution phases down to
//!   `bynk-project` would be a much larger, riskier refactor than this
//!   slice's own scope; keeping `ProjectGraph` beside `UnitTable`/
//!   `UnitSignature` (which already reuse `bynk-project`'s own `UnitKind`/
//!   `ParsedFile`) is the minimal, honest fix.

use std::collections::{BTreeMap, HashMap};

use bynk_project::{ParsedFile, UnitKind};
use bynk_syntax::span::FileId;

use crate::unit_signature::UnitId;

/// The reference's own three edge kinds (`design/bynk-greenfield-compiler.md`
/// §3.2): which commons a unit `uses`, which unit it `consumes` from, and
/// (per [DECISION C]) the dual of `Consumes` — which unit a provider serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EdgeKind {
    Uses,
    Consumes,
    Provides,
}

/// One unit's own identity, kind, and the files that contribute to it
/// (R3.7: "a unit may be contributed to by many files"). `files` carries the
/// real `FileId` the lexer stamped for each contributing file during this
/// analysis's own parse — not durable across analyses (that's P8.4's own
/// job, the shared `Tokens(FileId)`/`Ast(FileId)` cache), only stable for
/// the lifetime of the `ProjectGraph` that was built alongside it.
#[derive(Debug, Clone)]
pub struct Unit {
    pub id: UnitId,
    pub kind: UnitKind,
    pub files: Vec<FileId>,
}

/// P8.3 (#1514): the project-level query R3.13 names. [DECISION A] — no
/// `contract` field; see this module's own doc comment.
#[derive(Debug, Clone, Default)]
pub struct ProjectGraph {
    pub units: HashMap<UnitId, Unit>,
    /// Which unit a given file contributes to — the reverse of `Unit::files`,
    /// materialised because a diagnostic or a query typically starts from a
    /// file (a keystroke, an LSP request) and needs its owning unit, not the
    /// other way around.
    pub files: HashMap<FileId, UnitId>,
    pub edges: Vec<(UnitId, UnitId, EdgeKind)>,
}

/// Builds a [`ProjectGraph`] from the same resolved facts
/// `project_model.rs`'s own pipeline already computes for one analysis:
/// `groups`/`kinds` (phase 3, `phase_group`), `unit_uses` (phase 5,
/// `phase_resolve_uses`), `unit_consumes` (phase 5b,
/// `phase_resolve_consumes`, the first element of its pair return). Reading
/// already-resolved maps rather than re-deriving them from `parsed` is
/// deliberate — the whole point of this type is not to duplicate resolution
/// logic that already exists, only to materialise its result as one typed
/// value instead of several loose ones.
pub fn project_graph_for(
    parsed: &[ParsedFile],
    groups: &BTreeMap<String, Vec<usize>>,
    kinds: &BTreeMap<String, UnitKind>,
    unit_uses: &HashMap<String, Vec<String>>,
    unit_consumes: &HashMap<String, Vec<String>>,
) -> ProjectGraph {
    let mut units = HashMap::new();
    let mut files = HashMap::new();
    for (name, indices) in groups {
        let id = UnitId(name.clone());
        let kind = *kinds
            .get(name)
            .expect("phase_group's own groups/kinds maps share the same key set");
        // PR #1519's own bot review, finding #1: a synthetic first-party unit
        // (`bynk`, `bynk.map`, …) is tokenised via `lexer::tokenize`
        // (`firstparty_parsed`, `project_model.rs:320`), which stamps every
        // span — including the unit-name span this maps from —
        // `FileId::UNKNOWN`. Reading that span for every synthetic unit
        // would collide every one of them onto the same key in `files`
        // below (last insert silently wins), and would be dishonest anyway:
        // a synthetic unit has no on-disk file (`ParsedFile::abs_path` is
        // `None`), so an empty `files` list is the accurate answer, not a
        // sentinel FileId standing in for "no real file".
        let file_ids: Vec<FileId> = indices
            .iter()
            .filter(|&&i| !parsed[i].is_synthetic())
            .map(|&i| parsed[i].unit().name().span.file)
            .collect();
        for &fid in &file_ids {
            files.insert(fid, id.clone());
        }
        units.insert(
            id.clone(),
            Unit {
                id,
                kind,
                files: file_ids,
            },
        );
    }

    let mut edges: Vec<(UnitId, UnitId, EdgeKind)> = Vec::new();
    for (name, targets) in unit_uses {
        for target in targets {
            edges.push((UnitId(name.clone()), UnitId(target.clone()), EdgeKind::Uses));
        }
    }
    for (name, targets) in unit_consumes {
        for target in targets {
            edges.push((
                UnitId(name.clone()),
                UnitId(target.clone()),
                EdgeKind::Consumes,
            ));
            // [DECISION C]: the dual edge — `target` provides to `name`.
            edges.push((
                UnitId(target.clone()),
                UnitId(name.clone()),
                EdgeKind::Provides,
            ));
        }
    }
    // Deterministic order: `unit_uses`/`unit_consumes` are `HashMap`s, so
    // iteration order is not itself meaningful — sort so two builds from the
    // same resolved facts always produce the same `edges` vector.
    edges.sort();

    ProjectGraph {
        units,
        files,
        edges,
    }
}
