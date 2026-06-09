# Wrap a library as an adapter

You want to use an npm library (or a remote HTTP API) from Karn. Wrap it in an
**adapter**: declare the capability contract in Karn, implement it in a
TypeScript **binding**, and consume it like any other capability.

## 1. Declare the adapter

Name the adapter for the capability it provides. Declare the capability, any
boundary types, and an external (bodiless) `provides`. Name the binding module
and pin its npm dependency.

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

  provides Jwt = JoseJwt
}
```

## 2. Write the binding

The binding lives beside the adapter source at the path the `binding` clause
names. `implements Jwt` against the generated interface is the contract — `tsc
--strict` enforces it. Construct boundary values **through the emitted
constructors** (`Ok`/`Err`, the sum type's `JwtError.Invalid`, a `Claims` object
literal) — never hand-rolled tag shapes.

```typescript
// tokens.binding.ts
import * as jose from "jose";
import type { Jwt, Claims } from "./tokens.js";
import { JwtError } from "./tokens.js";          // emitted variant constructors
import { Ok, Err, type Result } from "./runtime.js";

export class JoseJwt implements Jwt {
  async sign(claims: Claims, secret: string): Promise<string> {
    return await new jose.SignJWT({ ...claims })
      .setProtectedHeader({ alg: "HS256" })
      .sign(new TextEncoder().encode(secret));
  }
  async verify(token: string, secret: string): Promise<Result<Claims, JwtError>> {
    try {
      const { payload } = await jose.jwtVerify(
        token,
        new TextEncoder().encode(secret),
      );
      return Ok({ sub: String(payload.sub), exp: Number(payload.exp) });
    } catch {
      return Err(JwtError.Invalid);
    }
  }
}
```

A **remote API** is the same shape with no npm dependency — drop the `requires`
clause and call `fetch` in the binding, mapping the response to a `Result`.

## 3. Consume it

```karn,ignore
context auth.sessions {
  consumes tokens { Jwt }      -- flatten `Jwt` into the local namespace

  service login {
    on call(secret: String) -> Effect[String] given Jwt {
      let token <- Jwt.sign(Claims { sub: "u1", exp: 0 }, secret)
      token
    }
  }
}
```

Compile: the adapter's interface module and the binding are emitted into the
output, the npm dependency is folded into `package.json`, and the composition
root instantiates the binding's class and injects it. To swap the real
implementation in a test, `mocks Jwt = … { … }` at the same seam.

## See also

- [Adapters reference](../../reference/adapters.md)
- [Adapter & binding errors](../troubleshooting/adapter-errors.md)
