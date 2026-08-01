# 0306 — The wire contract renders as a rung-1 hover appendix plus one new, narrowly-guarded hover rung — never by reordering the existing ladder

- **Status:** Accepted (v0.246)

**Context.** `bynk-lsp/src/hover.rs`'s module doc states the rung *order* is
the behaviour: #611's regression was a rung that resolved an offset correctly
but rendered nothing, so a later, name-matching rung answered instead — a
confidently wrong hover. Any change to hover for #855 had to preserve every
existing rung's `Some`/`None` behaviour exactly, changing only what *content*
already-`Some` answers carry, plus filling gaps no rung previously answered
at all.

**Decision.** Two additions, both content-preserving:

1. **Rung 1's appendix.** When the binding index resolves the cursor to a
   `Service` symbol whose declaration has exactly one handler, the
   wire-contract body (request envelope, cross-context form + hash, HTTP
   responses) is appended after whatever `describe_symbol` already rendered,
   separated by a `---` rule. Rung 1 still returns `Some` in exactly the same
   cases as before — only the string's tail changes, and a multi-handler
   service (where "which handler?" has no answer) gets no appendix, which is
   the pre-#855 behaviour unchanged.
2. **A new rung 4**, tried after the locals rung and before the lexical
   fallbacks, answering only the handler *header* — `on`, the HTTP
   method/handler-kind token, and any route string literal. Its guard is the
   safety property: `h.span.start <= offset < prefix_end`, where `prefix_end`
   is the first param's span start (or the body's span start with no
   params). That range contains no token any earlier or later rung can ever
   resolve as a symbol (`GET` lexes as a bare `Ident` with no index entry; no
   top-level declaration is named after a route literal), so the new rung is
   provably unable to shadow rung 1 — a param's *type* name
   (`client: ClientId`) always sits past `prefix_end` and keeps resolving to
   its declaration exactly as before #855.

**Consequences.** A new pinning test,
`hover_references.rs::header_rung_answers_the_header_and_never_shadows_a_params_type_name`,
asserts both halves of the invariant over the real `hover_content` (not a
replica of it): the header offset and the route-literal offset both answer
the wire contract, and a `ClientId`-in-params offset still resolves to the
type's own declaration, not the new rung — so the shadowing property is
pinned by a test, not merely argued in this ADR. It sits alongside, rather
than replacing, the existing rung-precedence pin
(`a_structural_rung_outranks_the_name_matching_locals_rung`). Lexical rungs
renumber 4→5 … 9→10 in the module doc and inline comments; their *content* is
untouched.
