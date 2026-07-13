# 0187 — Protocol sources are all lowercase: `from websocket`, not `from WebSocket`

- **Status:** Accepted (v0.161)
- **Provenance:** the keyword-hygiene batch (Bynk Language Design Review
  2026-07-05, §8 Language P1 #9, issue #548). The review flagged mixed protocol
  casing — `from http` / `from cron` / `from queue(…)` are lowercase, but the
  real-time source was `from WebSocket(…)` — and asked that the casing be
  normalised. Third item of the batch (after ADR 0185 `stub` and ADR 0186 `&&`).
- **Realises:** the four service-header protocol sources are spelled uniformly in
  lower case. A real-time service is now `service <N> from websocket(in: I, out: O)`.
- **Relates:** ADR 0131 (`from WebSocket` protocol bundle — the construct this
  renames the source of; frame binding, edge auth, and the `Connection` surface
  are unchanged), ADRs 0132–0135 (the Workers wire, hibernation, inbound, and
  broadcast slices — all describe `from WebSocket` as it shipped and stay as the
  historical record), ADR 0156 (editor surface tracks the language).

## Context

The `from <protocol>` clause on a service header admits a **closed, compiler-known
set** of sources. Three were lowercase — `http` and `cron` and `queue` are
reserved keyword tokens — while the fourth, `WebSocket`, was PascalCase and, unlike
the others, a **contextual identifier** (matched by text, not a reserved token, so
`WebSocket`/`in`/`out` stay usable as ordinary identifiers). The mixed casing was
the review's example of surface inconsistency: a reader has no rule that predicts
whether a protocol source is `lowercase` or `PascalCase`.

`WebSocket` came in PascalCase because it mirrors the JavaScript `WebSocket` global
the emission targets. But the Bynk source token and the JS runtime class are
different layers; `from http` does not capitalise to match `HTTP`, and neither
should the real-time source.

## Decision

**D1 — The protocol source is `websocket`.** `service <N> from websocket(in: I,
out: O)` replaces `from WebSocket(…)`. It stays a **contextual** word — not a
reserved keyword — exactly as before: `websocket` (and `in` / `out`) remain usable
as ordinary identifiers outside the header. The three sibling sources
(`http`/`cron`/`queue`) and the `on open` / `on message` / `on close` lifecycle
words are unchanged.

**D2 — Only the source token moves.** The internal AST variant stays
`ServiceProtocol::WebSocket` (Rust naming), the emitted TypeScript is byte-identical
(the JS `WebSocket` / `WebSocketPair` globals, the `101` upgrade, the Durable
Object mapping), and the `Connection[F]` held-resource surface is untouched.
"WebSocket" remains the proper-noun for the technology in prose and in descriptive
diagnostic text ("a WebSocket `on message` handler …"); only the literal source
keyword `from WebSocket` and the header shown in a message (`WebSocket(in: …)`)
lowercase.

**D3 — Breaking, and taken now.** `from WebSocket(…)` no longer parses (`websocket`
is matched instead). Pre-1.0 this is a one-word edit in the author's service
headers; the in-repo fixtures, examples, book, formatter rendering, and the LSP
`from`-completion candidate all migrate in this increment. The historical
changelog rows and ADRs 0131–0135 keep `from WebSocket` as the record of what those
versions shipped.

## Consequences

- All four protocol sources read uniformly lower-case; the "is it a keyword or a
  PascalCase name?" ambiguity is gone. A source is a lowercase word, full stop.
- `WebSocket` is freed as an identifier in every position (it was already
  contextual, so this is a spelling change, not a reservation change).
- The formatter emits `from websocket(…)`; hover/completion/semantic tokens track
  the lowercase spelling (ADR 0156). Signature help is unaffected (a protocol
  source is not a call site).
- The remaining keyword-hygiene items (#548) — the lexical tightening (`a--b`, the
  `---` divider) and the `where`-tier documentation, plus the docs-only enum
  convention — land separately.
