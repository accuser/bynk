//! Bynk's semantic-analysis layer — the crate between `bynk-syntax` and the
//! emitter.
//!
//! Holds name resolution (`resolver`), type checking (`checker`), the registries
//! the checker dispatches and the LSP reads (`kernel_methods`, `builtin_names`),
//! the first-party embedded sources (`firstparty`), actor analysis (`actors`),
//! and the **captured analysis tables** written during resolution/checking:
//! the binding `index`, inlay `hints`, `expr_types`, and `locals`. These tables
//! are produced here and *queried* by the IDE layer (`bynk-ide`, a later slice):
//! captured tables live in `bynk-check`, queries live above it (ADR 0102, and
//! the crate-decomposition track's check↔IDE seam).
//!
//! Extracted from `bynkc` as slice 3 of the crate-decomposition track. Behaviour
//! is unchanged; `bynkc` depends on this crate and re-exports its modules so its
//! public API and the emitter/project layers above are untouched.

pub mod actors;
pub mod builtin_names;
pub mod checker;
pub mod expr_types;
pub mod firstparty;
pub mod hints;
pub mod index;
pub mod kernel_methods;
pub mod locals;
pub mod requirements;
pub mod resolver;
pub mod store_ops;

pub use firstparty::Platform;
