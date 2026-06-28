// v0.102 (real-time track slice 2): the runtime contract for a held
// `Connection[F]`. A `Connection<F>` is a typed handle to a long-lived channel;
// `send` writes a server→client frame, `close` ends the connection. The
// concrete implementations — a capture-and-inspect `TestConnection` for the
// bundle/test target and the hibernatable-WebSocket binding for Workers — arrive
// with the `from WebSocket` protocol (slice 3); the language emits against this
// interface, and the linearity discipline (§2.9) governs ownership at the type
// level.
export interface Connection<F> {
  send(frame: F): Promise<void>;
  close(): Promise<void>;
}

// v0.103: the bundle/test realisation of a `Connection[F]` — a capture-and-inspect
// channel. The runtime-managed lifecycle in production becomes an inspectable
// record in tests (design notes §20): `sent` holds every frame `send`-given, and
// `closed` records disposal. A test drives an `on open` handler with a
// `TestConnection` and asserts what the agent sent.
export class TestConnection<F> implements Connection<F> {
  readonly sent: F[] = [];
  closed = false;

  async send(frame: F): Promise<void> {
    if (this.closed) throw new Error("send on a closed TestConnection");
    this.sent.push(frame);
  }

  async close(): Promise<void> {
    this.closed = true;
  }
}

// v0.104 (real-time track slice 3b): the Cloudflare Workers realisation of a
// `Connection[F]`, wrapping a server-side `WebSocket` accepted in a Durable
// Object. A frame is sent as JSON; `close` ends the socket. (This slice uses the
// non-hibernatable `server.accept()` model — the connection lives in the DO's
// memory and is lost on eviction; the hibernatable `acceptWebSocket` mapping that
// survives eviction is a follow-on increment.)
export class WorkersConnection<F> implements Connection<F> {
  constructor(private readonly ws: { send(data: string): void; close(): void }) {}

  async send(frame: F): Promise<void> {
    this.ws.send(JSON.stringify(frame));
  }

  async close(): Promise<void> {
    this.ws.close();
  }
}

// v0.104 (real-time track slice 3b): the minimal structural surface of a
// Cloudflare server-side `WebSocket`, so emitted Worker code type-checks under
// `tsc --strict` without depending on `@cloudflare/workers-types`. The real
// runtime object is richer but compatible.
export interface WorkersWebSocket {
  accept(): void;
  send(data: string): void;
  close(code?: number, reason?: string): void;
}

export interface WorkersWebSocketPair {
  client: WorkersWebSocket;
  server: WorkersWebSocket;
}

// Construct a Cloudflare `WebSocketPair` (a Workers runtime global), returned as
// a named `{ client, server }` pair. The pair is index-shaped at runtime
// (`pair[0]` = client, `pair[1]` = server); this normalises it and keeps the
// global access in one place so emitted code stays free of ambient declarations.
export function newWebSocketPair(): WorkersWebSocketPair {
  const Ctor = (globalThis as { WebSocketPair?: new () => { 0: WorkersWebSocket; 1: WorkersWebSocket } })
    .WebSocketPair;
  if (Ctor === undefined) {
    throw new Error("WebSocketPair is not available in this runtime");
  }
  const pair = new Ctor();
  return { client: pair[0], server: pair[1] };
}

// Build the `101 Switching Protocols` response that hands the client end of an
// accepted `WebSocketPair` back to the caller — the Cloudflare upgrade
// completion. `webSocket` is a Workers-specific `ResponseInit` extension.
export function webSocketUpgradeResponse(client: WorkersWebSocket): Response {
  return new Response(null, { status: 101, webSocket: client } as ResponseInit & {
    webSocket: WorkersWebSocket;
  });
}
