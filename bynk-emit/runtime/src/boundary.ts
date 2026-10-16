import type { Result } from "./result.ts";
import { Ok, Err } from "./result.ts";
import type { ValidationError } from "./errors.ts";

// v0.8: cross-Worker boundary protocol — JSON wire format and error types.

export type JsonValue =
  | null
  | boolean
  | number
  | string
  | JsonValue[]
  | { [k: string]: JsonValue };

export type BoundaryError =
  | { readonly kind: "MalformedJson"; readonly details: string }
  | {
      readonly kind: "StructuralMismatch";
      readonly path: string;
      readonly expected: string;
      readonly actual: string;
    }
  | {
      readonly kind: "RefinementViolation";
      readonly path: string;
      readonly violation: ValidationError;
    }
  | { readonly kind: "Transport"; readonly status: number; readonly details: string };

// v0.177 (#643): the deployed callee's compiled contract is not the one this
// caller was compiled against — a deploy skew, not a bad payload. The callee
// reports it as a 409 *before* reading the body, since once the contracts
// disagree the body's interpretation is exactly what is in doubt.
//
// Deliberately **not** a `BoundaryError` variant. `BoundaryError` is the
// *codec's* error domain — what deserialising a payload can conclude — and a
// codec can never produce this: it is decided from a header before any codec
// runs. Widening `BoundaryError` would oblige every consumer of a codec result
// (`Json.decode`'s error mapping among them) to narrow a case it can never
// observe. The call surface is wider than the codec surface, so it gets its own
// type.
export interface ContractMismatch {
  readonly kind: "ContractMismatch";
  readonly service: string;
  readonly expected: string;
  readonly actual: string | null;
}

/// Everything a cross-context call can fail with: the codec's domain, plus the
/// skew check that precedes it.
export type CallError = BoundaryError | ContractMismatch;

export function boundaryError(error: CallError): Error {
  const e = new Error(`BoundaryError: ${error.kind}`) as Error & {
    boundaryError?: CallError;
  };
  // #1825 review: non-enumerable, so a logged error (`console.error(..., e)`
  // in a Worker's fault catch) doesn't print the payload, which can carry a
  // value: a callee's response body, a refinement's offending value. Reading
  // `e.boundaryError` is unchanged.
  globalThis.Object.defineProperty(e, "boundaryError", {
    value: error,
    enumerable: false,
    writable: true,
    configurable: true,
  });
  return e;
}

// v0.96 (ADR 0124): an agent's persisted state failed validation on rehydration —
// a refined field, key, or entry no longer satisfies the current type definition
// (schema corruption, or a refinement that tightened across a deploy, orphaning
// previously-valid data). The load-time twin of InvariantViolation: a dedicated
// internal fault, NOT a caller-facing BoundaryError, because the supplier is
// trusted past-self, not the untrusted caller (Q6). It reuses the boundary
// validator's *detection* (the BoundaryError detail) but disposes of it as a
// fault. Logged with the agent type and field path only — never the key or the
// offending value (ADR 0107 logging discipline).
export interface RehydrationViolation {
  readonly kind: "RehydrationViolation";
  readonly agent: string;
  readonly path: string;
  readonly detail: BoundaryError;
}

export function rehydrationViolation(agent: string, detail: BoundaryError): Error {
  const path = "path" in detail ? detail.path : "<root>";
  // #1827: logged here, where every load-time gate builds it, as the
  // `InvariantViolation` line is logged before its throw: the agent, the field
  // path and the kind of failure. Never `detail` itself, which can carry the
  // offending value (a `RefinementViolation`'s `violation.value`).
  globalThis.console.error(`RehydrationViolation ${agent}`, { agent, path, kind: detail.kind });
  const e = new Error(`RehydrationViolation: ${agent} ${detail.kind} at ${path}`);
  // #1825 review: non-enumerable, as `boundaryError`'s, so a logged error
  // doesn't print `detail` (it can carry the offending value).
  globalThis.Object.defineProperty(e, "rehydrationViolation", {
    value: {
      kind: "RehydrationViolation",
      agent,
      path,
      detail,
    } satisfies RehydrationViolation,
    enumerable: false,
    writable: true,
    configurable: true,
  });
  return e;
}

export interface ServiceBinding {
  fetch(request: Request): Promise<Response>;
}

// I2 (#1971, ADR `per-edge-call-signature`, amending ADR 0092): every
// internal request (`/_bynk/call/`, `/_bynk/event/`) is signed with
// HMAC-SHA256 under a key one caller shares with one callee, and the callee
// refuses a request whose signature does not verify before it reads anything
// else. The path prefixes are reachable from the public internet on any route
// the Worker serves, so the prefix alone proves nothing about the sender, and
// one key per edge means a key's holder can claim only its own name. Bynk's
// emitter holds the same values in `bynk_check::wire_format`, and
// `bynkc/tests/wire_reference.rs` fails if the two disagree.
export const CALLER_HEADER = "X-Bynk-Caller";
export const TIMESTAMP_HEADER = "X-Bynk-Timestamp";
export const SIGNATURE_HEADER = "X-Bynk-Signature";
export const SIGNATURE_WINDOW_SECONDS = 300;
export const MAX_BODY_BYTES = 1048576;
export const SIGNING_SCHEME = "BYNK-CALL-1";
export const EDGE_FROM_PREFIX = "BYNK_CALL_FROM_";
export const EDGE_TO_PREFIX = "BYNK_CALL_TO_";

// A context name an edge key can be derived from: dot-separated segments of
// lowercase letters and digits, starting with a letter, with single
// underscores between runs. On these names `edgeSuffix` is injective, so one
// caller's key never verifies another caller's name.
const EDGE_NAME = /^[a-z][a-z0-9]*(?:_[a-z0-9]+)*(?:\.[a-z][a-z0-9]*(?:_[a-z0-9]+)*)*$/;

// `shop.orders` → `SHOP__ORDERS`, or `undefined` outside the domain above.
export function edgeSuffix(context: string): string | undefined {
  return EDGE_NAME.test(context) ? context.toUpperCase().split(".").join("__") : undefined;
}

// The edge key under `prefix` + the context's suffix in a Worker's `env`, or
// `undefined` when the name is outside the domain, or the key unset or not a
// non-empty string. `env` is `unknown` because the keys are secrets, not
// declared bindings, so no generated `Env` interface names them (the actor
// auth secrets read theirs the same way).
export function edgeKeyOf(env: unknown, prefix: string, context: string): string | undefined {
  const suffix = edgeSuffix(context);
  if (suffix === undefined || typeof env !== "object" || env === null) return undefined;
  const value = (env as Record<string, unknown>)[prefix + suffix];
  return typeof value === "string" && value !== "" ? value : undefined;
}

// I3 (#1971): a foreign caller's secret, by its declared name, from `env`.
function foreignKeyOf(env: unknown, name: string): string | undefined {
  if (typeof env !== "object" || env === null) return undefined;
  const value = (env as Record<string, unknown>)[name];
  return typeof value === "string" && value !== "" ? value : undefined;
}

// The fields a signature covers, one per line (`signing_input` in
// `bynk_check::wire_format`). Every header the callee acts on is here; an
// absent contract is signed as empty and an absent wire version as `1`.
export interface SignedFields {
  caller: string;
  callee: string;
  method: string;
  path: string;
  contract: string;
  wire: string;
  timestamp: string;
  bodyDigest: string;
}

export function signingInput(f: SignedFields): string {
  return [SIGNING_SCHEME, f.caller, f.callee, f.method, f.path, f.contract, f.wire, f.timestamp, f.bodyDigest].join("\n");
}

function hex(bytes: ArrayBuffer): string {
  return Array.from(new Uint8Array(bytes), (b) => b.toString(16).padStart(2, "0")).join("");
}

async function sha256Hex(bytes: Uint8Array<ArrayBuffer>): Promise<string> {
  return hex(await crypto.subtle.digest("SHA-256", bytes));
}

function hmacKey(secret: string, usage: "sign" | "verify"): Promise<CryptoKey> {
  return crypto.subtle.importKey("raw", new TextEncoder().encode(secret), { name: "HMAC", hash: "SHA-256" }, false, [usage]);
}

// The signature a caller holding `key` sends over `fields`.
export async function signature(key: string, fields: SignedFields): Promise<string> {
  return hex(await crypto.subtle.sign("HMAC", await hmacKey(key, "sign"), new TextEncoder().encode(signingInput(fields))));
}

// The sending side: the headers that prove `caller` sent `body` to `callee`.
// Throws, naming the unset key, when the Worker holds none for the edge:
// sending without it would only come back as an opaque 401.
export async function signedHeaders(
  env: unknown,
  caller: string,
  callee: string,
  path: string,
  contract: string,
  // The wire version as sent; `"1"` for a request that sends no header.
  wire: string,
  body: string,
  what: string,
): Promise<Record<string, string>> {
  const key = edgeKeyOf(env, EDGE_TO_PREFIX, callee);
  if (key === undefined) {
    throw new Error(
      `${EDGE_TO_PREFIX}${edgeSuffix(callee) ?? callee} is not set, so ${what} cannot be sent: \`bynk deploy\` sets one key per calling edge, and \`bynk dev\` passes a local one.`,
    );
  }
  const timestamp = String(Math.floor(Date.now() / 1000));
  const bodyDigest = await sha256Hex(new TextEncoder().encode(body));
  const sig = await signature(key, { caller, callee, method: "POST", path, contract, wire, timestamp, bodyDigest });
  return { [CALLER_HEADER]: caller, [TIMESTAMP_HEADER]: timestamp, [SIGNATURE_HEADER]: sig };
}

// The body's bytes, read under `MAX_BODY_BYTES`, or `undefined` once it is
// past the cap: a declared length is refused before reading, and a streamed
// body is cancelled as soon as it crosses the cap, so an unauthenticated
// sender cannot make the callee buffer an unbounded body.
async function readCapped(request: Request): Promise<Uint8Array<ArrayBuffer> | undefined> {
  const declared = request.headers.get("content-length");
  if (declared !== null && Number(declared) > MAX_BODY_BYTES) return undefined;
  if (request.body === null) return new Uint8Array(0);
  const reader = request.body.getReader();
  const chunks: Uint8Array[] = [];
  let size = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    size += value.byteLength;
    if (size > MAX_BODY_BYTES) {
      await reader.cancel();
      return undefined;
    }
    chunks.push(value);
  }
  const bytes = new Uint8Array(size);
  let at = 0;
  for (const c of chunks) {
    bytes.set(c, at);
    at += c.byteLength;
  }
  return bytes;
}

const SIGNATURE_SHAPE = /^[0-9a-f]{64}$/;
const TIMESTAMP_SHAPE = /^[0-9]{1,15}$/;

// The receiving side, the first thing every internal dispatch does: the body
// as text when `request` is signed for `callee` by a caller this Worker holds
// a key for, within the window; otherwise the refusal to return, a bare 413
// or 401. Fails closed: a caller with no key here (an undeclared or forged
// one, or any caller when no key is set) is refused. `crypto.subtle.verify`
// compares in constant time.
//
// I3 (#1971): `foreign` maps each foreign caller the callee admits (a
// `Foreign` actor's name, which it sends as its caller) to the secret it signs
// under. Such a name is never a context's (an actor name starts with a capital
// letter, which `edgeSuffix` refuses), so a foreign caller is checked under
// its own secret and a Bynk caller under its edge key, and neither can pass
// for the other. Looked up with `Object.hasOwn`, never plain indexing, so a
// caller named after an `Object.prototype` member finds nothing.
export async function verifyCall(
  request: Request,
  env: unknown,
  callee: string,
  foreign: Record<string, string> = {},
): Promise<string | Response> {
  const bytes = await readCapped(request);
  if (bytes === undefined) return new Response("Payload Too Large", { status: 413 });
  const refused = new Response("Unauthorized", { status: 401 });
  const caller = request.headers.get(CALLER_HEADER) ?? "";
  const key = Object.hasOwn(foreign, caller) ? foreignKeyOf(env, foreign[caller]) : edgeKeyOf(env, EDGE_FROM_PREFIX, caller);
  if (key === undefined) return refused;
  const timestamp = request.headers.get(TIMESTAMP_HEADER) ?? "";
  if (!TIMESTAMP_SHAPE.test(timestamp)) return refused;
  if (Math.abs(Date.now() / 1000 - Number(timestamp)) > SIGNATURE_WINDOW_SECONDS) return refused;
  const given = request.headers.get(SIGNATURE_HEADER) ?? "";
  if (!SIGNATURE_SHAPE.test(given)) return refused;
  const sig = new Uint8Array(32);
  for (let i = 0; i < 32; i++) sig[i] = parseInt(given.slice(2 * i, 2 * i + 2), 16);
  const fields: SignedFields = {
    caller,
    callee,
    method: request.method,
    path: new URL(request.url).pathname,
    contract: request.headers.get("X-Bynk-Contract") ?? "",
    wire: request.headers.get(WIRE_VERSION_HEADER) ?? "1",
    timestamp,
    bodyDigest: await sha256Hex(bytes),
  };
  const ok = await crypto.subtle.verify(
    "HMAC",
    await hmacKey(key, "verify"),
    sig,
    new TextEncoder().encode(signingInput(fields)),
  );
  return ok ? new TextDecoder().decode(bytes) : refused;
}

// #1969: the published cross-context wire format's version
// (`reference/wire-format.md`). A caller states the version it speaks; a callee
// accepts a range of them and treats an absent header as version 1. Bynk's
// emitter holds the same values in `bynk_check::wire_format`, and
// `bynkc/tests/wire_reference.rs` fails if the two disagree.
export const WIRE_VERSION_HEADER = "X-Bynk-Wire";
export const WIRE_VERSION = "1";

export async function callService<R>(
  binding: ServiceBinding,
  servicePath: string,
  argsJson: JsonValue,
  // #988: generic over the whole return type `R`, not a `Result`'s two
  // halves — `R = Result<T, E>` recovers the old behaviour exactly, but a
  // plain (non-`Result`) `R` now has something to bind `deserialiseResult`'s
  // inference to, instead of degrading to `unknown`. The `path` parameter is
  // optional here because every generated `deserialise_*` declares it with a
  // default, which callers of this function are not required to match.
  deserialiseResult: (json: JsonValue, path?: string) => Result<R, BoundaryError>,
  // v0.54: the calling context's qualified name, stamped beside the args so the
  // callee's `by c: Caller` handler can present a live `CallerId` (Q7). A
  // compile-time constant; the args body itself is unchanged. The `Internal`
  // channel trusts the binding, so this is identity, not authentication.
  callerContext: string = "",
  // v0.177 (#643): this caller's compiled hash of the callee's contract. A
  // compile-time constant, stamped beside the caller identity as metadata — the
  // args body is untouched. The callee compares it against its own constant and
  // fails closed on mismatch (409), which is what makes a `deploy --context`
  // skew a loud, nameable failure instead of a silent misinterpretation.
  contractHash: string = "",
  // I2 (#1971): the calling Worker's `env`, which holds the key for the edge
  // to `calleeContext` ([`EDGE_TO_PREFIX`]), and the callee's qualified name,
  // which the signature binds the request to.
  env: unknown = undefined,
  calleeContext: string = "",
): Promise<R> {
  const body = JSON.stringify(argsJson);
  const path = `/_bynk/call/${servicePath}`;
  const signed = await signedHeaders(env, callerContext, calleeContext, path, contractHash, WIRE_VERSION, body, `the call to ${servicePath}`);
  const request = new Request(`http://internal/_bynk/call/${servicePath}`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      "X-Bynk-Caller": callerContext,
      "X-Bynk-Contract": contractHash,
      [WIRE_VERSION_HEADER]: WIRE_VERSION,
      ...signed,
    },
    body,
  });
  const response = await binding.fetch(request);
  if (!response.ok) {
    // Read the body **once**: the stream is consumed on first read, so a second
    // read throws `TypeError: Body is unusable` — which would replace the very
    // diagnosis this increment exists to give with an opaque failure from inside
    // the runtime.
    const raw = await response.text();
    // v0.177 (#643): a 409 from the internal boundary is the callee refusing a
    // skewed contract. Surface it as the named `ContractMismatch` rather than a
    // generic transport failure — the whole point of failing closed is that the
    // operator learns *what* is wrong, and "409 with an opaque body" would bury
    // it. Anything else — including a 409 that is not ours, or one whose body
    // does not parse — stays a `Transport`.
    if (response.status === 409) {
      let detail: ContractMismatch | null = null;
      try {
        detail = JSON.parse(raw) as ContractMismatch;
      } catch {
        // Not a Bynk 409; fall through to `Transport`.
      }
      if (detail && detail.kind === "ContractMismatch") {
        // #1826: logged on the caller's side too, with both hashes, before the
        // fault: the caller's catch reports only `BoundaryError: ContractMismatch`.
        globalThis.console.error(`ContractMismatch ${callerContext} -> ${servicePath}`, detail);
        throw boundaryError(detail);
      }
    }
    throw boundaryError({
      kind: "Transport",
      status: response.status,
      details: raw,
    });
  }
  let responseJson: JsonValue;
  try {
    responseJson = (await response.json()) as JsonValue;
  } catch (e) {
    throw boundaryError({ kind: "MalformedJson", details: String(e) });
  }
  const result = deserialiseResult(responseJson);
  if (result.tag === "Err") {
    throw boundaryError(result.error);
  }
  return result.value;
}

// Events track, slice 0: the fan-out DO's own delivery to one subscriber,
// over that subscriber's Service Binding. Subscriber failure isolation (ADR
// 0284) is the caller's responsibility — the fan-out DO catches per-subscriber
// so one subscriber's rejection does not stop delivery to its siblings; this
// helper itself just throws on a non-`ok` response.
//
// Slice 2 (spine #936): the POST body is `{ payload, envelope }`, not the
// bare payload — the envelope is always sent; whether the receiving
// subscriber's handler actually declared the optional `env: EventEnvelope`
// parameter is decided entirely on the receiving side (the `/_bynk/event/`
// route in the emitted `index.ts`), which forwards it to the handler only
// when that handler's own generated wrapper expects it.
export async function deliverEvent(
  binding: ServiceBinding,
  servicePath: string,
  payload: unknown,
  envelope: unknown,
  // I2 (#1971): the fan-out Durable Object's `env`, which holds the key for
  // the edge from the publishing context to the subscriber, and the two
  // contexts' qualified names. The subscriber verifies before it reads.
  env: unknown = undefined,
  publisher: string = "",
  subscriber: string = "",
): Promise<void> {
  const body = JSON.stringify({ payload, envelope });
  const path = `/_bynk/event/${servicePath}`;
  const signed = await signedHeaders(env, publisher, subscriber, path, "", "1", body, `the event delivery to ${servicePath}`);
  const request = new Request(`http://internal/_bynk/event/${servicePath}`, {
    method: "POST",
    headers: { "content-type": "application/json", ...signed },
    body,
  });
  const response = await binding.fetch(request);
  if (!response.ok) {
    throw new Error(`event delivery to ${servicePath} failed: ${response.status}`);
  }
}

// #973: `EventEnvelope` (`{ eventId, publisherId, emittedAt, schemaVersion }`)
// is declared in `adapter bynk`, not by a consumed context, so it never goes
// through the ordinary `emit_record_codec` machinery a context's own boundary
// types get (that machinery is rooted at a *consumed context's* exports;
// widening it to cover every adapter-declared type would touch every context
// that imports any `bynk` type, for one envelope). This hand-written validator
// is its sole codec, checked at the receiving `/_bynk/event/` route
// (`workers_entry.rs`) exactly once per delivery, unconditionally — the
// envelope is always on the wire regardless of whether a given subscriber's
// handler declared the optional `env: EventEnvelope` parameter. Field rules
// mirror the generated `Int`/`Instant` codec rule (`serialisation.rs`'s
// `TypeRef::Base` arm): both wire as `number`, integer-checked.
export function deserialiseEventEnvelope(
  json: JsonValue,
  path: string = "$",
): Result<
  { eventId: string; publisherId: string; emittedAt: number; schemaVersion: number },
  BoundaryError
> {
  if (typeof json !== "object" || json === null || Array.isArray(json)) {
    return Err({ kind: "StructuralMismatch", path, expected: "object", actual: typeof json });
  }
  const obj = json as { [k: string]: JsonValue };
  const eventId = obj["eventId"];
  if (typeof eventId !== "string") {
    return Err({
      kind: "StructuralMismatch",
      path: `${path}.eventId`,
      expected: "string",
      actual: typeof eventId,
    });
  }
  const publisherId = obj["publisherId"];
  if (typeof publisherId !== "string") {
    return Err({
      kind: "StructuralMismatch",
      path: `${path}.publisherId`,
      expected: "string",
      actual: typeof publisherId,
    });
  }
  const emittedAt = obj["emittedAt"];
  if (typeof emittedAt !== "number" || !Number.isInteger(emittedAt)) {
    return Err({
      kind: "StructuralMismatch",
      path: `${path}.emittedAt`,
      expected: "integer",
      actual: typeof emittedAt,
    });
  }
  const schemaVersion = obj["schemaVersion"];
  if (typeof schemaVersion !== "number" || !Number.isInteger(schemaVersion)) {
    return Err({
      kind: "StructuralMismatch",
      path: `${path}.schemaVersion`,
      expected: "integer",
      actual: typeof schemaVersion,
    });
  }
  return Ok({ eventId, publisherId, emittedAt, schemaVersion });
}
