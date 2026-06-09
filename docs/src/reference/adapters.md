# Adapters

An **adapter** is the one declaration kind where a Karn capability *contract*
sits adjacent to a non-Karn *implementation*. It is the **only** place the host
boundary may exist — the single, named, greppable seam through which a
deploy-target runtime or an npm library enters a Karn program. Everything else
stays pure Karn.

An adapter declares capabilities and the boundary types they reference, names a
TypeScript **binding** that supplies the implementations, and `exports` the
capabilities to consumers. It may **not** declare services or agents, and its
providers are **external** (bodiless).

## Anatomy

```karn,ignore
adapter tokens {
  binding "./tokens.binding.ts" requires { "jose": "^5" }

  exports capability  { Jwt }
  exports transparent { Claims, JwtError }

  type Claims   = { sub: String, exp: Int }
  type JwtError = enum { Invalid, Expired }

  capability Jwt {
    fn sign(claims: Claims, secret: String) -> Effect[String]
    fn verify(token: String, secret: String) -> Effect[Result[Claims, JwtError]]
  }

  provides Jwt = JoseJwt        -- external: no body; supplied by the binding
}
```

- **`binding "<module>"`** names the TypeScript module (resolved relative to the
  adapter's source file) that exports the provider symbols. `requires { … }`
  declares npm dependencies; ranges must be pinned (no `*`/`latest`).
- **`provides Cap = Name`** with **no brace block** is an *external* provider:
  the compiler emits no class, and the binding must `export class Name implements
  Cap`. The `implements` is checked by `tsc --strict` — that is the contract
  between the two halves.

## The three flavours

| Flavour | Binding | Portability |
|---|---|---|
| **Library adapter** | one, npm-backed, user-authored | runs anywhere |
| **The `karn` surface** | one per platform, toolchain-supplied | portable |
| **Vendor adapter** | one, vendor-only | platform-locked |

The **`karn` surface** is the reserved, agnostic conformance core
(`Clock`, `Random`, `Logger`, …) shipped with the toolchain; consuming only
`karn` keeps code portable. The `karn` root namespace is reserved — no user unit
may be named `karn` or `karn.*`.

## Consuming an adapter

A context `consumes` an adapter exactly as it consumes another context. Selected
capabilities can be flattened to bare names:

```karn,ignore
context auth.sessions {
  consumes karn   { Logger }   -- portable
  consumes tokens { Jwt }      -- library adapter; bare `Jwt` in scope

  service login {
    on call(secret: String) -> Effect[String] given Jwt, Logger {
      let _     <- Logger.info("issuing token")
      let token <- Jwt.sign(Claims { sub: "u1", exp: 0 }, secret)
      token
    }
  }
}
```

`consumes U { Cap, … }` flattens the named capabilities into the consumer's local
namespace, so they read as `given Cap` / `Cap.op(…)` — identical to a locally
declared capability. The emitted TypeScript is the same as the qualified
`given U.Cap` form.

A consumed adapter is wired **in-process** (its binding is instantiated in the
composition root), never over a Service Binding — an adapter is not a deployment
unit.

## The binding as privileged constructor

A binding constructs its adapter's boundary types, which deliberately pierces
Karn's construction discipline (only the defining unit may construct a type).
Inside a binding that rule does not apply — the binding *is* the host boundary.
To avoid coupling to the emitter's lowering, bindings construct boundary values
**only through the emitted constructors** — `Ok`/`Err`/`Some`/`None` from
`runtime.js`, a sum type's `T.Variant`, a record as an object literal, and a
**refined type through its validating `.of`** (handling the `Result`; a raw cast
or `.unsafe` bypasses the predicate and is disallowed).

## See also

- [Wrap a library as an adapter](../how-to/adapters/wrap-a-library.md)
- [Capabilities & providers](capabilities.md)
- [Adapter & binding errors](../how-to/troubleshooting/adapter-errors.md)
