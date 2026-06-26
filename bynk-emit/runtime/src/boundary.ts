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

export function boundaryError(error: BoundaryError): Error {
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
): Promise<Result<T, E>> {
  const request = new Request(`http://internal/_bynk/call/${servicePath}`, {
    method: "POST",
    headers: { "content-type": "application/json", "X-Bynk-Caller": callerContext },
    body: JSON.stringify(argsJson),
  });
  const response = await binding.fetch(request);
  if (!response.ok) {
    throw boundaryError({
      kind: "Transport",
      status: response.status,
      details: await response.text(),
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
