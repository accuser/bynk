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

// #2004 (ADR 0092 amended): the deployment's shared call secret. Every Worker
// in a deployment holds the same value under this binding, every internal
// request (`/_bynk/call/`, `/_bynk/event/`) carries it in this header, and
// the receiving Worker refuses a request without it before reading anything
// else. The path prefixes are reachable from the public internet on any route
// the Worker serves, so the prefix alone proves nothing about the sender.
export const CALL_SECRET_BINDING = "BYNK_CALL_SECRET";
export const CALL_SECRET_HEADER = "X-Bynk-Call-Secret";

// The secret from a Worker's `env`, or `undefined` when it is unset or not a
// non-empty string. `env` is `unknown` because the binding is a secret, not a
// declared binding, so no generated `Env` interface names it (the actor auth
// secrets read theirs the same way).
export function callSecretOf(env: unknown): string | undefined {
  if (typeof env !== "object" || env === null) return undefined;
  const value = (env as Record<string, unknown>)[CALL_SECRET_BINDING];
  return typeof value === "string" && value !== "" ? value : undefined;
}

// Logged once per isolate, not once per request: an unset secret refuses
// every internal request, and anyone can send those.
let callSecretUnsetLogged = false;

// Does `request` carry this Worker's call secret? Fails closed: an unset
// secret on the receiving side refuses everything. The comparison runs over
// SHA-256 digests of both values, so it takes the same time whatever the
// header holds, its length included; `crypto.subtle.timingSafeEqual` would
// do, but it is workerd-only and the generated test harness runs on Node.
export async function verifyCallSecret(request: Request, env: unknown): Promise<boolean> {
  const expected = callSecretOf(env);
  if (expected === undefined) {
    if (!callSecretUnsetLogged) {
      callSecretUnsetLogged = true;
      globalThis.console.error(
        `${CALL_SECRET_BINDING} is not set: refusing every internal call to this Worker. \`bynk deploy\` sets it; \`bynk dev\` passes a local one.`,
      );
    }
    return false;
  }
  const given = request.headers.get(CALL_SECRET_HEADER) ?? "";
  const encoder = new TextEncoder();
  const [a, b] = await Promise.all([
    crypto.subtle.digest("SHA-256", encoder.encode(given)),
    crypto.subtle.digest("SHA-256", encoder.encode(expected)),
  ]);
  const x = new Uint8Array(a);
  const y = new Uint8Array(b);
  let diff = 0;
  for (let i = 0; i < x.length; i++) diff |= x[i] ^ y[i];
  return diff === 0;
}

// The sending side: the secret to stamp, or a thrown error naming the unset
// binding. Sending without it would only come back as an opaque 401.
function requireCallSecret(env: unknown, what: string): string {
  const secret = callSecretOf(env);
  if (secret === undefined) {
    throw new Error(
      `${CALL_SECRET_BINDING} is not set, so ${what} cannot be sent: \`bynk deploy\` sets it on every Worker, and \`bynk dev\` passes a local one.`,
    );
  }
  return secret;
}

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
  // #2004: the calling Worker's `env`, for the deployment's call secret
  // ([`CALL_SECRET_BINDING`]), which the callee checks before anything else.
  env: unknown = undefined,
): Promise<R> {
  const secret = requireCallSecret(env, `the call to ${servicePath}`);
  const request = new Request(`http://internal/_bynk/call/${servicePath}`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      "X-Bynk-Caller": callerContext,
      "X-Bynk-Contract": contractHash,
      [CALL_SECRET_HEADER]: secret,
    },
    body: JSON.stringify(argsJson),
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
  // #2004: the fan-out Durable Object's `env`, for the call secret the
  // subscriber checks before it reads the body.
  env: unknown = undefined,
): Promise<void> {
  const secret = requireCallSecret(env, `the event delivery to ${servicePath}`);
  const request = new Request(`http://internal/_bynk/event/${servicePath}`, {
    method: "POST",
    headers: { "content-type": "application/json", [CALL_SECRET_HEADER]: secret },
    body: JSON.stringify({ payload, envelope }),
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
