// @karn/runtime — v0.2
//
// The minimum surface used by code emitted by karnc. From v0.2 onward
// Result and Option are discriminated unions on `tag` so that user-
// defined sum types lower to the same shape and `match` / `is` work
// uniformly across all of them.

export type Result<T, E> =
  | { readonly tag: "Ok"; readonly value: T }
  | { readonly tag: "Err"; readonly error: E };

export const Ok = <T>(value: T): Result<T, never> => ({ tag: "Ok", value });
export const Err = <E>(error: E): Result<never, E> => ({ tag: "Err", error });

export type Option<T> =
  | { readonly tag: "Some"; readonly value: T }
  | { readonly tag: "None" };

export const Some = <T>(value: T): Option<T> => ({ tag: "Some", value });
export const None: Option<never> = { tag: "None" };

export interface ValidationError {
  readonly field: string;
  readonly message: string;
  readonly value: unknown;
}
