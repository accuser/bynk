//! Which `runtime.ts` helpers an emitted module actually references.
//!
//! Most of the runtime import list is fixed (`Ok`, `Err`, `Some`, `None`, …),
//! but three groups are conditional, because importing them unconditionally
//! would put names in every module that most modules never use:
//!
//! - the `Bytes` helpers (`__bynkBytesEqual` and friends, v0.110 / ADR 0142),
//!   emitted by the `Bytes` kernel, `==` on `Bytes`, and the boundary codecs;
//! - the ICU formatters (`selectPluralArm`, `formatIcuNumber`, `formatIcuDate`,
//!   #878), emitted by a `messages` bundle's `render`.
//!
//! The condition is "did emission actually reference it", which is a fact only
//! emission knows. This type is how that fact travels: the producers call
//! [`RuntimeUse::note_bytes`] / [`RuntimeUse::note_icu`] as they emit, and the
//! header/post-pass reads the flags to decide the import list.
//!
//! # Why not scan the generated text
//!
//! This replaces `out.contains("__bynkBytes")` and friends. Scanning the output
//! for the helper's own name is wrong in both directions. It over-matches: the
//! generated text also carries user-authored string literals, doc comments, and
//! field names, any of which could contain the marker and pull in an import
//! nothing calls. Worse, it under-matches: `out.contains("selectPluralArm(")`
//! quietly depends on the call being emitted with no space before its paren, so
//! a formatting change in an unrelated part of the emitter silently drops a
//! required import and produces a module that does not compile. There is no test
//! that would obviously catch it, because the marker and the scan live in
//! different files.
//!
//! The same reasoning is already recorded a few lines from the code this
//! replaces: `HttpResult`'s import is decided structurally (over the AST)
//! "so a comment or string literal mentioning `HttpResult` never triggers a
//! spurious import" (`file_mentions_http_result`). This extends that principle
//! to the two conditions that were still text scans.
//!
//! # What keeps the producers honest
//!
//! A producer added without a matching `note_*` call emits a module that
//! references an unimported name. Deliberately *not* guarded here by comparing
//! the flags against a scan of the generated text: that scan would inherit the
//! over-matching described above, so a user string literal containing the marker
//! would fire the assertion on valid input — trading a rare missing import for a
//! panic on legitimate code. The check belongs where it is already total:
//! `bynkc`'s fixture suite type-checks every emitted fixture under `tsc --strict`
//! (`tests/tsc_verify.rs`), which fails loudly on an unimported reference.
//!
//! Interior mutability (`Cell`) rather than `&mut`: the producers sit at the
//! bottom of a deep tree of `&`-taking emit functions — several of them pure
//! `String`-returning helpers — and a shared `&RuntimeUse` threads through that
//! tree without turning every intermediate signature into a `&mut` chain.

use std::cell::Cell;

/// Per-module accumulator of conditional runtime-helper references.
///
/// One instance per emitted module (it lives on that module's emission context),
/// so flags never leak between files.
#[derive(Default)]
pub struct RuntimeUse {
    bytes: Cell<bool>,
    icu: Cell<bool>,
}

impl RuntimeUse {
    /// Record that the module references the `Bytes` runtime helpers.
    pub fn note_bytes(&self) {
        self.bytes.set(true);
    }

    /// Record that the module references the ICU formatting helpers.
    pub fn note_icu(&self) {
        self.icu.set(true);
    }

    /// Whether the `Bytes` helpers must be imported.
    pub fn bytes(&self) -> bool {
        self.bytes.get()
    }

    /// Whether the ICU formatting helpers must be imported.
    pub fn icu(&self) -> bool {
        self.icu.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_start_clear_and_latch() {
        let u = RuntimeUse::default();
        assert!(!u.bytes());
        assert!(!u.icu());
        u.note_bytes();
        assert!(u.bytes());
        assert!(!u.icu(), "noting one helper does not set the other");
        u.note_icu();
        assert!(u.icu());
        // Latching: a second note is not a toggle.
        u.note_bytes();
        assert!(u.bytes());
    }

    /// A `&RuntimeUse` is all a producer needs — the point of the `Cell`.
    #[test]
    fn a_shared_reference_can_record() {
        fn producer(u: &RuntimeUse) {
            u.note_bytes();
        }
        let u = RuntimeUse::default();
        producer(&u);
        assert!(u.bytes());
    }
}
