import type { Result } from "./result.ts";
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
  const e = new Error(`BoundaryError: ${error.kind}`);
  (e as any).boundaryError = error;
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
  const e = new Error(`RehydrationViolation: ${agent} ${detail.kind} at ${path}`);
  (e as { rehydrationViolation?: RehydrationViolation }).rehydrationViolation = {
    kind: "RehydrationViolation",
    agent,
    path,
    detail,
  };
  return e;
}

export interface ServiceBinding {
  fetch(request: Request): Promise<Response>;
}

export async function callService<T, E>(
  binding: ServiceBinding,
  servicePath: string,
  argsJson: JsonValue,
  deserialiseResult: (json: JsonValue) => Result<Result<T, E>, BoundaryError>,
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
): Promise<Result<T, E>> {
  const request = new Request(`http://internal/_bynk/call/${servicePath}`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      "X-Bynk-Caller": callerContext,
      "X-Bynk-Contract": contractHash,
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
      if (detail && detail.kind === "ContractMismatch") throw boundaryError(detail);
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
