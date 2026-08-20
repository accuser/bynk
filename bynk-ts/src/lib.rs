//! The TypeScript tree and printer (phase 7 of the compiler trajectory,
//! `design/bynk-compiler-trajectory.md`). Emission produces nodes
//! ([`TsProgram`]/[`TsStmt`]); [`printer::print`] is the only code in the
//! compiler that writes a character (R7.3).
//!
//! **Invariant:** this crate depends on `bynk-syntax` **only** (for
//! [`bynk_syntax::span::Span`], reused unchanged rather than redefined —
//! `bynk-emit`'s own `TypeRef`/`Ir`
//! machinery is not visible here, and never will be: a function taking one
//! would not compile, the dependency isn't present, by design). `cargo tree
//! -p bynk-ts` shows `bynk-syntax` and nothing else. Matches the same
//! load-bearing shape `bynk-render`'s own module doc states for itself
//! (`bynk-render/src/lib.rs`).
//!
//! **What exists here today (P7.5, #1307) — and what doesn't yet.** The
//! reference's own §7.1 sketch names four tree enums (`TsStmt`/`TsExpr`/
//! `TsType`/`TsDecl`) with real variants (`Const`, `Let`, `Binary`, `Named`,
//! `Class`, …). Only [`TsStmt`] exists here, and it has exactly one variant:
//! [`VerbatimOrigin`]-tagged escape-hatch text (Q2, `design/tracks/
//! the-typescript-tree.md` §3.2). `bynk-emit` builds no [`TsProgram`] yet —
//! Arc C's own first slice is what starts converting real emission into real
//! nodes, file by file; this crate exists so that conversion has somewhere
//! to land, following R10.3's own "carve prospectively, at the moment the
//! dependency appears" precedent (`bynk-strip`'s own carve is the control
//! case cited for this). The full `TsStmt`/`TsExpr`/`TsType`/`TsDecl` sketch
//! — no `Any`/`enum`/`namespace`/decorator/param-property representable —
//! is P7.8's own job, gated on this crate existing at all.

mod lint;
mod printer;
mod program;
mod source_map;

pub use lint::{Violation, verbatim_violations};
pub use printer::{Printed, print};
pub use program::{TsProgram, TsStmt, VerbatimOrigin};
pub use source_map::SourceMapBuilder;
