//! Which `runtime.ts` helpers an emitted module actually references.
//!
//! Most of the runtime import list is fixed (`Ok`, `Err`, `Some`, `None`, …),
//! but four groups are conditional, because importing them unconditionally
//! would put names in every module that most modules never use:
//!
//! - the `Bytes` helpers (`__bynkBytesEqual` and friends, v0.110 / ADR 0142),
//!   emitted by the `Bytes` kernel, `==` on `Bytes`, and the boundary codecs;
//! - the ICU formatters (`selectPluralArm`, `formatIcuNumber`, `formatIcuDate`,
//!   #878), emitted by a `messages` bundle's `render`;
//! - the boundary-codec group (`Ok`, `Err`, `Result`, `BoundaryError`, #914),
//!   which an *inlined* deserialiser builds directly;
//! - the JSON-codec group (`Ok`, `Err`, `Result`, `JsonValue`, `JsonError`,
//!   #914), which the `Json.decode[T]` wrapper names in its own signature and
//!   body, independently of which arm the inner deserialiser takes.
//!
//! Most modules import `Ok`/`Err`/`Result` unconditionally and never consult the
//! last two; they exist for the module kinds that curate their runtime import
//! list — a Worker's `compose.ts` and the test-scaffold modules.
//!
//! #917 adds a fifth, differently-shaped piece of state, riding this same
//! shared reference for the same reason: a test-scaffold module's `Json.decode`/
//! `Json.encode` on a named type has no local codec to delegate to (the unit it
//! imports exports the type but not a codec for it), so the module generates its
//! own — the `json_codec_qual`/`json_codec_roots` fields carry the namespace a
//! foreign type's TS positions render through and the roots its bodies reached
//! for, respectively. Not a runtime-*helper* reference like the four flags
//! above, but the same "note during lowering, decide after" shape.
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
//!
//! The consequence, recorded here so it does not have to be bisected for later:
//! `Cell` is `!Sync`, so [`RuntimeUse`] is `!Sync`, and so is `EmitProjectCtx`
//! now that it owns one — an `&EmitProjectCtx` can no longer cross a thread
//! boundary. Nothing in the tree is parallel today, so this costs nothing, but
//! per-unit emission is the obvious thing to parallelise, and that is when it
//! will bite. Swapping the fields to `AtomicBool` with `Relaxed` ordering
//! restores `Sync` at no meaningful cost and leaves every call site reading the
//! same — do that rather than unpicking the shared-`&` design.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use bynk_syntax::ast::TypeRef;

/// Per-module accumulator of conditional runtime-helper references.
///
/// One instance per emitted module (it lives on that module's emission context),
/// so flags never leak between files.
#[derive(Default)]
pub(crate) struct RuntimeUse {
    bytes: Cell<bool>,
    icu: Cell<bool>,
    boundary_codec: Cell<bool>,
    json_codec: Cell<bool>,
    /// #917: for a test-scaffold module, the type-only namespace each
    /// codec-eligible named type reaches through (`"Order" -> "orders."`) — the
    /// module has no local declaration of its own, only a namespace import of
    /// the suite's target (and its `uses`). Empty (the default) on every other
    /// emission path, where every name renders bare.
    json_codec_qual: RefCell<HashMap<String, String>>,
    /// #917: the `Json.decode[T]`/`Json.encode` target type-refs a test-scaffold
    /// module's case/property/stub bodies actually reached for, collected as they
    /// lower. `emit_test_module` drains this once all bodies are lowered to
    /// compute the codec closure those bodies need — the unit module they import
    /// exports the type but no codec, so a test-scaffold module generates its own
    /// (#661's caller-generates-its-own-codec pattern, applied to test scaffolding).
    json_codec_roots: RefCell<Vec<TypeRef>>,
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

    /// Record that the module inlines a boundary deserialiser, which builds
    /// `Ok(…)` / `Err(… as BoundaryError)` directly rather than delegating to a
    /// `deserialise_<T>` in its own namespace.
    pub fn note_boundary_codec(&self) {
        self.boundary_codec.set(true);
    }

    /// Record that the module emits the `Json.decode[T]` wrapper, whose
    /// signature and body name `Result`, `JsonValue` and `JsonError` regardless
    /// of which arm the inner deserialiser takes.
    pub fn note_json_codec(&self) {
        self.json_codec.set(true);
    }

    /// #917: set once, up front, before any body lowers — the type-only
    /// namespace qualifier a test-scaffold module's codec calls render foreign
    /// named types through. A no-op default (empty map) everywhere else.
    pub fn set_json_codec_qual(&self, qual: HashMap<String, String>) {
        *self.json_codec_qual.borrow_mut() = qual;
    }

    /// The namespace qualifier a test-scaffold module's codec calls render
    /// foreign named types through (empty on every other emission path).
    pub fn json_codec_qual(&self) -> std::cell::Ref<'_, HashMap<String, String>> {
        self.json_codec_qual.borrow()
    }

    /// #917: record a `Json.decode[T]`/`Json.encode` target type-ref a
    /// test-scaffold body reached for, so the module that spliced it in can
    /// generate the codec helper its delegating call needs.
    pub fn note_json_codec_root(&self, t: TypeRef) {
        self.json_codec_roots.borrow_mut().push(t);
    }

    /// Drain the accumulated `Json` codec roots — called once, after every
    /// case/property/stub body in the module has lowered.
    pub fn take_json_codec_roots(&self) -> Vec<TypeRef> {
        std::mem::take(&mut *self.json_codec_roots.borrow_mut())
    }

    /// Whether the `Bytes` helpers must be imported.
    pub fn bytes(&self) -> bool {
        self.bytes.get()
    }

    /// Whether the `Ok` / `Err` / `Result` / `BoundaryError` group must be
    /// imported.
    pub fn boundary_codec(&self) -> bool {
        self.boundary_codec.get()
    }

    /// Whether the `Ok` / `Err` / `Result` / `JsonValue` / `JsonError` group
    /// must be imported.
    pub fn json_codec(&self) -> bool {
        self.json_codec.get()
    }

    /// Whether the ICU formatting helpers must be imported.
    pub fn icu(&self) -> bool {
        self.icu.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The type's contract, written down: the flags are independent and a
    /// note latches. Everything else about this type is `Cell` behaviour and is
    /// not worth a test — the coverage that matters drives the real producers
    /// end-to-end (`emitter::conditional_runtime_import_tests`).
    #[test]
    fn notes_are_independent_and_latch() {
        let u = RuntimeUse::default();
        u.note_bytes();
        assert!(u.bytes());
        assert!(!u.icu(), "noting one helper does not set the other");
        u.note_bytes();
        assert!(u.bytes(), "a second note is not a toggle");
    }
}
