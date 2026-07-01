---
title: "Format your code with `bynk-fmt`"
---
**Goal:** format Bynk source to the canonical style.

Bynk's formatter is built into the compiler as `bynkc fmt`.

## Format files in place

```sh
bynkc fmt src/counters.bynk
bynkc fmt src/*.bynk
```

This rewrites the named files to canonical form (tab indentation, normalised
spacing). For example:

```bynk
commons demo {
type Id=Int
fn add(a:Int,b:Int)->Int{a+b}
}
```

becomes:

```bynk
commons demo {
	type Id = Int

	fn add(a: Int, b: Int) -> Int { a + b }
}
```

## Format via stdin

Pass `-` to read from stdin and write to stdout — handy for editor integrations:

```sh
cat src/counters.bynk | bynkc fmt -
```

## Check formatting in CI

`--check` verifies formatting without writing, exiting non-zero if any file is
not already canonical:

```sh
bynkc fmt --check src/*.bynk
```

## Related

- [Set up editor support](/docs/editor-and-tooling/editor-support/) for format-on-save.
- Reference: [`bynk-fmt`](/docs/tooling/bynk-fmt/).
