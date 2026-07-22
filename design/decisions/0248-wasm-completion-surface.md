# 0248 — The playground gains completion via a bynk_complete wasm entry

- **Status:** Accepted (v0.216.3)

**Context.** ADR "completion-logic-to-bynk-ide" moved `bynk-lsp`'s pure
completion logic into `bynk-ide`, reachable from `wasm32-unknown-unknown`.
This increment adds the wasm entry and playground wiring that logic
unblocked, completing #808 (the other half of #397, after ADR 0242 shipped
hover).

`bynk-lsp`'s own `completion` handler folds two further contexts in
handler-side, outside `completion::complete()` itself: in-scope
locals/params (`bynk_check::locals::locals_at`, gated on a keyword or
expression cursor position) and value-receiver `.member` completion
(retyping a rewritten buffer via `expr_types`, when `complete()` itself
yields nothing). Both bottom out in primitives `bynk-wasm`'s hover entry
already uses (`analyse_in_memory_with_types` + `type_at_offset`) — the wasm
side just needs a single-buffer version, with no multi-doc overlay or
analysis-round caching, since the playground has exactly one buffer and no
project files.

**Decision.** `bynk_emit::project::InMemoryAnalysis` gains a `locals` field
(`Vec<bynk_check::locals::LocalBinding>`), populated from the same
`RunChecks` result `expr_types` already drains, mirroring exactly how
`expr_types` was added for hover. `bynk-wasm` gains `complete`/
`complete_to_json`/`bynk_complete`, following `bynk_hover`'s three-layer
shape (inner fn → `catch_panic` → `..._to_json` → `#[wasm_bindgen]`):
`complete()` calls `bynk_ide::completion::complete()`, then appends locals
at a keyword/expression position, then falls back to
`value_receiver_rewrite` + `value_member_candidates` when the first two
contexts yield nothing. `bynk-ide`'s `Completion`/`CompletionKind` stay
serde-free (a pure analysis crate); `bynk-wasm` defines its own
`CompletionCandidate` wire DTO, the same pattern as its existing
`EmittedFile`/`Diagnostic` types. The playground adds
`@codemirror/autocomplete` and wires a `CompletionSource` calling
`bynk_complete`, alongside the existing `hoverTooltip`/linter extensions.

**Deliberately out of scope**, matching how ADR 0242 scoped hover down to
the bare inferred type rather than `bynk-lsp`'s richer hover ladder: the
`cors`/`security`/`cache`/`limits`/`@limit` annotation-field completions and
`is`/`match`-arm scrutinee-variant completions all stay LSP-only — they
either build `tower_lsp::lsp_types::CompletionItem` directly or need
rewrite-and-retype machinery beyond `value_member_candidates`. The issue's
own framing ("capability methods, types, keywords") is satisfied by
`complete()` + locals + value-member completion alone.

`insert_text` (LSP snippet syntax, `${1:label}`/`$0`, only populated for
`Snippet`-kind items) is not translated to CodeMirror's own snippet
placeholder grammar this round — every candidate's `apply` is its bare
label. A follow-up can add the small syntax translator and switch snippet
items to `snippetCompletion`.

**Consequences.** The playground editor now offers completion as you type.
Byte offsets match the existing hover/diagnostic convention. Annotation-field
and pattern-match completions, and snippet-syntax translation, remain
follow-ups.
