// @karn/runtime — v0
//
// The minimum surface used by code emitted by karnc.

export type Result<T, E> =
  | { readonly ok: true; readonly value: T }
  | { readonly ok: false; readonly error: E };

export const Ok = <T>(value: T): Result<T, never> => ({ ok: true, value });
export const Err = <E>(error: E): Result<never, E> => ({ ok: false, error });

export interface ValidationError {
  readonly field: string;
  readonly message: string;
  readonly value: unknown;
}
