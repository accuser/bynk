---
level: minor
changelog: Hover and a VS Code "Show Wire Contract" panel surface a handler's request envelope, its cross-context contract hash, and each boundary type's re-validation strategy — derived from a new shared IR the emitter's own codec generation now renders too
---

## ADR: wire-contract-boundary-sites
title: The wire-contract peek covers HTTP and cross-context `on call` handlers; agent/queue/WebSocket handlers are deferred
summary: DECISION A — the issue's two worked examples define the shipped scope; other boundary kinds are a later slice

**Context.** A Bynk project crosses a trust boundary at several sites: an
HTTP route, a cross-context `on call`, an agent's own `on call` (a
Durable-Object RPC boundary), a queue `on message`, and a WebSocket
`on open`/`on close`. The issue's worked examples name exactly two of these —
an HTTP route (the rate-limiter's `GET /check/:client`) and a cross-context
`on call` — and `bynk_check::wire::WireModel` (the shared IR both the codec
and the peek render) makes no distinction between boundary kinds: it derives
purely from a handler's params/return type and the type table, so nothing in
the IR itself limits which handler kinds could show a peek.

**Decision.** `bynk_ide::wire_contract` is scoped to **service handlers
only**. An agent's `on call` handler crosses a boundary too, but
`ContextBoundaryInfo` (the retained per-round table this module reads) does
not retain agents' own handler bodies in a form this module needs, and
extending it was out of scope for getting the issue's two worked examples
correct end-to-end. `BoundaryKind` (the peek's own protocol-kind enum) already
carries `Cron`/`Message`/`Open`/`Close`/`Event` variants alongside `Http` and
`Call` — a handler of any of those kinds gets a wire-contract model (request
envelope, boundary types), just never a `NoCrossContextReason` distinct from
`NotACallHandler`/`SingleContext`, since only `on call` has a cross-context
contract form at all.

**Consequences.** An agent-handler peek, and any queue-/WebSocket-specific
framing the panel might eventually want (e.g. a queue's redelivery semantics
rendered beside the envelope), is left for a later slice if requested — the
IR underneath does not need to change to add one, only
`bynk_ide::wire_contract`'s handler-resolution walk (`wire_contract_at`,
currently `CommonsItem::Service` only) and the LSP's header-rung guard
(`hover.rs`'s `header_handler_at`, likewise service-scoped).

## ADR: wire-contract-presentation
title: The wire contract renders as a rung-1 hover appendix plus one new, narrowly-guarded hover rung — never by reordering the existing ladder
summary: DECISION B — append, don't reorder; the new rung's byte range provably cannot shadow rung 1

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

## ADR: wire-contract-shared-shape
title: One IR (`bynk_check::wire::WireModel`) backs both the emitter's codec generation and the peek; `contract.rs`'s canonical hash form stays a separate, deliberately un-unified derivation
summary: DECISION C, shipped form — two consumers of one IR, not three; folding in contract.rs was investigated and rejected on evidence

**Context.** Before this increment, a boundary type's wire shape existed only
as a control-flow path through `bynk-emit`'s `serialisation.rs` — `TypeRef`
in, `writeln!`-ed TypeScript out, no intermediate value. A peek written
against that would necessarily re-derive the shape by hand, and a re-
derivation drifts from the original the moment either one changes without the
other. `bynk-check` already has a second boundary-canonicalisation pass —
`contract.rs`'s `service_normal_form`, which computes a cross-context
contract's hash — and an early pass considered folding the codec's shape into
that same canonical form as a third consumer, so there would be exactly one
"the shape of a boundary type" function in the crate.

**Decision.** That folding was investigated and **rejected on evidence**: the
two canonicalisations disagree on every axis that matters, each correctly for
its own job. `contract.rs:248` documents predicate sorting as a
*precondition* for hash correctness — hashing predicates in declaration order
"would make two contexts that agree perfectly fail closed against each
other," since the hash's whole job is that semantically-equal contracts hash
equal. `wire.rs`'s codec-generation IR needs the opposite: `Inline`
revalidation emits one `if` per predicate in **declaration order**, because
that is the order a JS `if`-chain (or a `switch` on sum variants, or a
record's emitted JSON keys) is observably rendered in. The same reversal
holds for record fields (sorted-by-name vs. declaration order) and for an
opaque type's predicate (elided from the hash — unobservable to a consumer
that cannot see it — but present in the IR, since the *owner* still
re-validates it). Unifying the two forms would either break the hash's
false-positive guarantee or misrender the codec — there is no shared
"canonical" form that is correct for both jobs simultaneously.

`contract.rs` is therefore **left untouched** by this increment. What the two
derivations genuinely share — and what would silently break if a future
change made `wire.rs`'s boundary walk and `contract.rs`'s reach a different
*set* of named types for the same handler, even though they render/order
that set completely differently — is asserted directly:
`bynk-check/src/wire.rs`'s `boundary_reachability_agrees_with_contract_normal_form`
test walks both derivations from the same handler and asserts equal
*reachability* (which types each one visits), never equal string form or
order.

**Consequences.** `bynk_check::wire::WireModel` has exactly **two**
consumers: `bynk-emit`'s codec generation (a byte-identical rewrite over the
IR, proven by an unchanged `bynkc/tests/fixtures` corpus) and
`bynk-ide::wire_contract` (the peek). A third derivation
(`contract.rs`'s hash) continues to exist deliberately unfolded, pinned by
its own reachability cross-check rather than by shared code.

## ADR: wire-contract-derivation-source
title: The wire-contract IR derives from the AST + type table, not `checker::Ty`
summary: DECISION D — `Ty` is lossy for refinement predicates, `contract.rs` cannot depend on checker output, and the peek must work on files with errors

**Context.** `bynk-check`'s checker produces `Ty::Named { name, kind, args }`
for a named type — the representation everything downstream of type-checking
already uses. Deriving the wire-contract IR from `Ty` instead of from the raw
`TypeDecl` + type table was the more obvious-looking option, since it is
already the checker's own settled vocabulary for "what type is this."

**Decision.** Derive from the AST (`TypeRef`/`TypeDecl`) plus the type table
instead, mirroring exactly how `contract.rs` already derives its own
canonical form. Two independent reasons converge on the same answer:

1. **`Ty` is lossy for what this IR needs.** `Ty::Named` carries a type's
   name, its `kind`, and its generic `args` — never its refinement
   predicates. Deriving `WireScalar::predicates`/`revalidation` from `Ty`
   would still require the identical type-table lookup this module already
   does directly; `Ty` would buy only generic-argument substitution, which
   the moved-verbatim walks (`collect_type_names`, `subst_type_ref`, …)
   already implement over `TypeRef` without it.
2. **`contract.rs` sets the precedent this module must not break.**
   `contract.rs` lives in `bynk-check` and derives its own canonical form
   from the AST + type table directly — it cannot depend on the checker
   having run, since a checker pass over one file cannot see another
   context's declarations at all in single-file mode. `wire.rs` keeps the
   same shape of dependency for the same reason, and for a reason `contract.rs`
   does not share: the peek (`bynk_ide::wire_contract`'s `wire_contract_at`)
   must answer for a file with **errors** — the whole point of a hover peek
   is that it works while the author is mid-edit, and a `checker::Ty` derived
   only where the checker ran clean would go blank exactly when the author
   most wants to see the boundary shape.

**Consequences.** The IR construction functions
(`wire_ref`/`wire_type`/`boundary_model`) take `&HashMap<String, Arc<TypeDecl>>`,
never a `Ty` or an `expr_types` table. Where the peek genuinely does need a
checker fact it cannot get any other way — disambiguating a bare `Ok(_)`
literal from an ordinary same-shaped value for the HTTP response walk — it
reads `expr_types` directly and **degrades to the declared-return-type
heuristic** when that table is empty (ADR 0063's clean-file ceiling), rather
than pulling the whole IR through a checker dependency it does not otherwise
need. That fallback is documented at its one call site
(`bynk_ide::wire_contract::ResponseWalk::is_http_result_expr`) and in
`design/bynk-lsp-spec.md` §3.26, rather than left to look exact when it is
not.
