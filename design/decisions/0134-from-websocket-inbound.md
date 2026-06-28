# 0134 — `from WebSocket` inbound: `on message`/`on close` are DO-hosted service handlers that decode against `in:` and recover the sender from the socket attachment

- **Status:** Accepted (real-time track, slice 3b-iii; 2026-06-28).
- **Provenance:** the inbound-dispatch piece ADR 0133 deferred ("`webSocketMessage(ws, msg)` decoding against `in:` and routing to a handler — a new protocol surface"). Slices 3a/3b-i/3b-ii built the **outbound** path (an authenticated `on open` transfers a `Connection` to an agent that `send`s to it, surviving hibernation). 3b-iii adds the **receive/close** half: the `on message` and `on close` service handlers the §20 design commits to, completing the bidirectional contract. **Security-bearing** (a client now drives agent logic over the channel): ran `/security-review` + `/code-review`.
- **Realises:** design notes §7 / §20 (the chat-room's `on message`/`on close`). Consumes the hibernation attachment of [ADR 0133](0133-from-websocket-hibernation.md) and the DO-hosted-handler + self-agent lowering of [ADR 0132](0132-from-websocket-protocol-workers.md).
- **Relates:** [ADR 0130](0130-held-resource-linearity.md) (the held-resource discipline — the firing `connection` is a **borrowed** held binding here).

## Context

The `in: ClientFrame` type has been declared since slice 3, but no handler
consumed it — a client could not talk back. The §20 design notes are explicit
about the shape: `on message`/`on close` are **service** handlers (like `on
open`), authenticated by the same `by` actor, that `match` the decoded frame and
dispatch to the agent's ordinary `on call` methods. So variant routing is just a
`match` in the body — no new routing machinery. The work is the *plumbing*: decode
the inbound frame, recover the sender, run the body in the hosting DO.

## Decisions

- **D1 — `on message`/`on close` are DO-hosted; identity + route args come from the
  socket attachment.** They run in the connection-hosting Durable Object (the same
  one `on open` resolved to), lowered with the self-agent rewrite (an agent transfer
  is a `this`-self-call). The `by` identity and the route values are **not**
  re-derived from the frame and **not** re-verified per message — they were fixed at
  `on open`, when the edge authenticated the actor, and are carried in the socket's
  `serializeAttachment` (extending 3b-ii's `{ connId }` to `{ connId, identity, args
  }`). Server-side, so not client-forgeable; the socket is authenticated **once**.

- **D2 — the inbound frame is decoded against `in:`, fail-closed.** Cloudflare's
  `webSocketMessage(ws, message)` runs the raw message through the boundary
  deserialiser for the service's `in` type; a structurally-invalid or
  refinement-violating frame **closes the socket** (`1003` unreadable/malformed,
  `1008` invalid) and is **never dispatched**. The decode is the only place client
  bytes are trusted — the §7 "frames are typed messages" model.

- **D3 — the firing `connection` is a borrowed held binding.** In `on message`/`on
  close`, `connection` is the socket the frame/close arrived on (re-wrapped from
  `ws` with the attachment connId). It is **borrowed**, not owned: the handler may
  `send` to it (non-consuming) but may not `close`/transfer it
  (`bynk.held.consume_on_borrow`), and it carries no disposal obligation at scope
  exit. (Contrast `on open`'s connection, which is owned and must be disposed.)

- **D4 — the `on message` frame is identified by type.** An `on message` handler has
  exactly one parameter typed as the service's `in` (the decoded frame,
  `bynk.ws.message_frame_param` otherwise); the remaining parameters are route
  values recovered positionally from the attachment args. This reuses the existing
  single-parameter-list parser — no two-list syntax change. At most one `on
  message` and one `on close` per service.

- **D5 — `on close` is an optional domain hook.** A service may declare `on close`
  (e.g. `Room(roomId).leave(…)`, which `remove`s the stored connection); it is not
  required. A stored connection whose socket has closed already resolves to `None`
  (3b-ii fail-soft), so its absence is a stale map entry, not a live leak. No runtime
  auto-prune at v1 — one disposal path.

- **D6 — bundle: callable handlers, test-driven.** Off Workers there is no
  `webSocketMessage`; `on message`/`on close` lower to callable surface methods a
  test drives with a `TestConnection` and a constructed frame — symmetric with `on
  open`. The §20 chat-room's inbound echo runs green under node on the bundle target.

## Internal architecture

- **Syntax:** `HandlerKind::Close` (new); a `from WebSocket` `on message` reuses
  `HandlerKind::Message`, disambiguated from the queue consumer by the service
  protocol.
- **Check:** the synthetic `connection` is injected for `on message`/`on close`
  (borrowed, via a new `borrowed_held` set threaded into the linearity pass); the
  frame-param-by-type and ≤1-each arity rules.
- **Emit:** the open/message/close DO methods share one body emitter (self-agent
  lowering); the DO gains `webSocketMessage`/`webSocketClose` that decode + recover
  the attachment + dispatch; the `on open` accept writes `{ identity, args }` into
  the attachment when the service has an inbound/close handler.
- **Runtime:** `acceptHibernatableConnection` gains an optional attachment-meta
  argument.

## Deferred (named, not dropped) — the slice-4 closure

- **Broadcast-to-all-connections** (`connections.values.parTraverse(c => c.send(…))`)
  — the held-aware iteration borrow surface, deferred since slice 2. 3b-iii's inbound
  vertical replies to the *sender's* connection (a single `send`), not a fan-out.
- The full §20 chat-room running end-to-end (presence, `Idempotency`, `Events`,
  fan-out) — slice 4, the track's completion proof.

## Consequences

- The bidirectional channel is complete: a client can send frames that decode and
  drive agent logic, and the socket's close runs a domain hook.
- The §20 chat-room is more complete on both targets — bundle fixture 236 gains the
  inbound echo + close (driven green by the behaviour test); Workers fixture 238
  emits the `webSocketMessage`/`webSocketClose` dispatch + the `__wsMessage`/
  `__wsClose` bodies, `tsc --strict`-clean.
- No real-`webSocketMessage` runtime proof in the unit harness (needs
  Miniflare/workerd); coverage is the bundle behaviour test + the Workers shape
  snapshot + `tsc --strict` + the node strip-types guard.
