# What is Karn?

Karn is a statically typed programming language for building services. It is
**architecture-first**: the shape of a program — its contexts, services,
agents, and the types that flow between them — is part of the language, not a
convention layered on top. Karn compiles to **typed TypeScript** and targets
**Cloudflare Workers**.

## The idea in one example

A minimal Karn program declares a context and a type:

```karn
commons demo {
  type Id = Int
}
```

A small HTTP service is not much larger:

```karn
context greet

service api {
  on http GET "/ping" () -> Effect[HttpResult[String]] {
    Ok("pong")
  }
}
```

Compiling either with `karnc` produces TypeScript you can read, run, and deploy.

## What makes Karn distinct

- **Make illegal states unrepresentable.** Karn leans on *refined types*
  (types carrying a predicate), *opaque types*, and *errors-as-values*
  (`Result`, `Ok`/`Some`/`None`) so that whole classes of bug cannot be
  expressed. See [The type-system philosophy](../explanation/type-system-philosophy.md).
- **Architecture in the language.** Contexts, services, and stateful *agents*
  are first-class. See [How a Karn program is shaped](../explanation/how-a-karn-program-is-shaped.md).
- **Compiles to TypeScript.** You get JavaScript-ecosystem interop and a
  natural fit for Cloudflare Workers, with a static type system in front of it.
  See [Why compile to TypeScript](../explanation/why-compile-to-typescript.md).
- **Testing is built in.** `test` blocks, `assert`, dependency `mocks`, and
  `Mock[T]` value fabrication ship with the language.

## What Karn is *not* (yet)

Karn is pre-1.0. Some designed features — events, sagas, and storage kinds —
are **deferred, not missing**, and land in later increments on the road to v1.
This book documents only what compiles today and marks planned features as
planned. See [Versioning & roadmap](../explanation/versioning-and-roadmap.md).

## Next steps

- [Install Karn](install.md)
- [Compile your first program](../tutorials/01-first-program.md)
