# `karn.adapter.*` / `karn.namespace.*` / `karn.requires.*` errors

These diagnostics relate to **adapters** — the host boundary. See the
[Adapters reference](../../reference/adapters.md) and
[Wrap a library as an adapter](../adapters/wrap-a-library.md).

## `karn.adapter.provider_has_body`

```text
[karn.adapter.provider_has_body] a provider inside an `adapter` must be external (no body) — its implementation is supplied by the binding
```

Providers inside an adapter are **external**: write `provides Cap = Name` with no
brace block. The implementation lives in the binding, not in Karn.

```karn,ignore
provides Jwt = JoseJwt        -- not `provides Jwt = JoseJwt { fn sign(…) { … } }`
```

## `karn.context.external_provider`

```text
[karn.context.external_provider] an external (bodiless) provider is only allowed inside an `adapter` — a context provider must have a Karn body
```

A bodiless `provides` is only legal inside an adapter. In a context, give the
provider a Karn body — or move it to an adapter if it wraps host code.

## `karn.adapter.disallowed_item`

```text
[karn.adapter.disallowed_item] an `adapter` may not declare a `service`
```

Adapters contain only capabilities, boundary types, external providers, inline
pure helpers, and `exports`. Put services and agents in a `context`.

## `karn.adapter.no_binding`

```text
[karn.adapter.no_binding] adapter `tokens` declares an external provider but has no `binding` clause to supply its implementation
```

An adapter with external providers must name a `binding` module:

```karn,ignore
binding "./tokens.binding.ts"
```

The same code reports when the named module cannot be read — check the path
(resolved relative to the adapter's source file) and author the `.binding.ts`.

## `karn.requires.unpinned_dependency`

```text
[karn.requires.unpinned_dependency] dependency `jose` has an unpinned version range `*` — pin a concrete range (e.g. `^1.2.0`)
```

Pin every dependency range. Unpinned ranges (`*`, `latest`, or anything with no
version number) make builds irreproducible and are rejected.

## `karn.namespace.reserved`

```text
[karn.namespace.reserved] `karn.time` uses the reserved `karn` namespace — the `karn` root is reserved for the toolchain's conformance surface
```

The `karn` root namespace names the toolchain's conformance surface. Rename any
user `commons`/`context`/`adapter`/`test` so its first segment is not `karn`.

## `karn.consumes.capability_name_clash`

```text
[karn.consumes.capability_name_clash] flattened capability `Jwt` clashes with a capability declared locally — use qualified `given tokens.Jwt` instead
```

A `consumes U { Cap }` flattened name collided with a local capability or another
flattened name. Resolve it with a qualified `given U.Cap`, or alias the unit with
`consumes U as Alias`.
