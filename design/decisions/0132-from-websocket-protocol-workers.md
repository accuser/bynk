# 0132 — the `from WebSocket` Workers wire path: authenticate at the edge, accept into the hosting Durable Object; held connections live in-memory; hibernation re-association is deferred

- **Status:** Accepted (real-time track, slice 3b-i; 2026-06-28).
- **Provenance:** the Cloudflare Workers half of the real-time / WebSocket track's slice 3 — slice 3a (ADR 0131) shipped the protocol surface, the security-critical checker rules, and a runnable bundle vertical against `TestConnection`; this slice lands the Workers production wire path. The slice was split again at implementation: **3b-i (here)** lands a working, edge-authenticated upgrade using the non-hibernatable `server.accept()` model; **3b-ii** lands hibernation re-association. **Security-bearing:** ran `/security-review` (the edge-auth path) + `/code-review`. Settles the track's **Q7** (hibernation) only in shape (the re-association is 3b-ii) and the Workers runtime half of **Q8** (the subprotocol auth read).
- **Realises:** design notes §7 (the WebSocket protocol on Workers — "the server socket is accepted into the addressed Durable Object") and §2.9.6 (a stored connection survives hibernation — a platform-supplied guarantee the language relies on but does not implement); consumes the `Connection[F]` type + linearity pass of [ADR 0130](0130-held-resource-linearity.md) and the protocol surface of [ADR 0131](0131-from-websocket-protocol-bundle.md).
- **Relates:** [ADR 0017](0017-platform-lock-per-deployment-unit.md) (the platform-lock 3a set, removed here); [ADR 0109](0109-agent-storage-staged-commit.md) (the agent state record + staged commit a held map is split out of); the actor track (the `by` clause + Bearer/None schemes the edge reuses, now reading the carrier at runtime).

## Context

A live `WebSocket` cannot be passed over a Durable Object RPC, and Cloudflare
hibernation requires the socket to be accepted by the DO that hosts it. So the
design note's sentence — "the upgrade happens in the Worker, the server socket is
accepted into the addressed Durable Object" — hides a topology problem: the
upgrade *request* is what moves between the Worker and the DO, not the socket.
This is also the track's safety boundary: a wrong shape lets a client hold a live
channel into an agent before the actor has verified. 3b-i makes edge-auth-before-
accept runtime-real; the largest deferred piece (hibernation re-association) is
named, not silently dropped.

## Decisions

- **D1 (the topology, the security crux) — authenticate in the Worker, accept into
  the DO.** The `on open` body runs **in the Durable Object** that hosts the
  connection (where, on Workers, the agent is local). The Worker authenticates and
  **forwards the upgrade request** to that DO only on success; the DO accepts the
  socket and runs the body. **No unauthenticated request reaches the DO, no socket
  is accepted before auth.** The verified identity (and validated route args) ride
  edge→DO in a trusted internal header (`X-Bynk-Ws-Open`) — `Headers.set`
  overwrites any client-supplied value, so it is not forgeable; the DO is reachable
  only through the Worker, the same internal-channel trust the `/_bynk/agent/`
  caller seam already relies on.

- **D2 — the on-open shape constraint pins the routable target.** The hosting DO is
  resolved **statically** from the **single connection transfer** the `on open`
  makes (`Room(roomId).join(…, connection)` → the `ROOM` namespace, keyed by
  `roomId`). A handler with zero / multiple / non-routable transfers is the compile
  error `bynk.ws.open_transfer_shape`. Inside the DO the transfer lowers to a
  **`this`-self-call** (the connection is already here; it never crosses an RPC).

- **D3 (Q8 runtime half) — the Bearer token is read from `Sec-WebSocket-Protocol`,
  verified at the edge before forwarding.** A browser sets it via `new
  WebSocket(url, [token])`. The edge reads the first comma-separated element,
  verifies with the **same audited `verifyBearerJwtHs256`** HTTP uses (fail-closed →
  `401`, do not forward), runs a refinement actor's authorization predicate (`403`),
  and **validates each route param through its `.of` constructor** (`400`, exactly
  as the HTTP path validates a path param) before the value addresses a DO or is
  forwarded. The actor's scheme may be `None` (an intentional anonymous channel,
  mirroring an HTTP `by v: Visitor` route); a present seam is always Bearer
  (`Signature` is rejected at the WS boundary by ADR 0131 D3).

- **D4 — a held `store Map[K, Connection]` is in-memory on Workers, split out of the
  persisted record.** A live socket cannot be JSON-persisted with the rest of agent
  state, so on Workers a held connection map lives in an **in-memory side-table**
  (`heldStore`, keyed by the durable state object so all instances addressing a key
  share it; entry ops lower to a JS `Map`). It is plain isolate memory: it survives
  for the DO's lifetime and is **lost on eviction** — the **non-hibernatable**
  lifecycle. The persisted state record, zero, rehydration gate, and load/commit
  exclude it. (On the bundle target a held map keeps its current tested behaviour in
  the in-memory test state record — the split is Workers-only.)

- **D5 — the upgrade route is a query-string convention for v1.** An `Upgrade:
  websocket` request routes to the context's `from WebSocket` service; route params
  (the on-open's `roomId`, …) are read from the upgrade URL's **query string** by
  name (a missing required param is a `400`). One WS service per context at v1;
  per-path disambiguation of multiple WS services is a named follow-on.

- **D6 — the runtime `WorkersConnection<F>`.** `send(frame)` JSON-encodes the frame
  to `ws.send`; `close()` ends the socket. The DO `fetch` builds it over the
  server end of a `WebSocketPair` (runtime helpers `newWebSocketPair` /
  `webSocketUpgradeResponse` keep the Cloudflare globals out of emitted modules, so
  the output type-checks under `tsc --strict` with no `@cloudflare/workers-types`).

## Deferred (3b-ii and the slice-4 closure), named not dropped

- **Hibernation re-association (Q7).** `state.acceptWebSocket(server, [connId])` +
  `serializeAttachment`; on wake `getWebSockets(connId)` re-presents the socket and
  the stored `Connection` re-wraps it — entirely in the runtime binding, behind the
  `Connection<F>` interface (the language keeps "stored value in, stored value
  out"). 3b-i uses `server.accept()` (non-hibernatable): the connection lives in the
  DO's memory and is lost on eviction.
- **Inbound frame dispatch.** `webSocketMessage(ws, msg)` decoding against `in:` and
  dispatching to the agent's handlers; `webSocketClose` disposing the held
  connection. 3b-i hosts a single connection's open lifecycle; inbound is 3b-ii.
- **Broadcast-to-all-connections** (`connections.values …`) — the held-aware
  iteration borrow surface, the slice-4 closure.

## Consequences

- A `from WebSocket` service compiles for `--target workers`; the 3a platform-lock
  `bynk.target.websocket_workers_unsupported` is removed.
- The §20 chat-room emits and type-checks under `tsc --strict` on Workers (positive
  fixture `237_websocket_chatroom_workers`); the transfer-shape constraint is pinned
  by negative fixture `259_ws_open_transfer_shape`.
- The dead on-open service-surface method is no longer emitted on Workers (the edge
  wrapper replaces it); held-map agents (e.g. fixture 235) move their connection map
  off the persisted record.
