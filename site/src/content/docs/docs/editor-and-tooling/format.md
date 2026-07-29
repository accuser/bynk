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

## Override the style for a run

Bynk has one canonical style, and the default output is it. When a house style
or a narrower terminal genuinely calls for something else, four flags override
the style for that invocation:

```sh
bynkc fmt --indent spaces --indent-width 4 src/*.bynk
bynkc fmt --max-line-width 120 src/*.bynk
bynkc fmt --no-trailing-comma src/*.bynk
```

| Flag | Default | Effect |
|---|---|---|
| `--indent tab\|spaces` | `tab` | Tabs are the default so each reader picks their own width in their editor. |
| `--indent-width N` | `2` | Spaces per nesting level, with `--indent spaces`. Passing it with `--indent tab` is an error rather than a silent no-op. |
| `--max-line-width COLUMNS` | `100` | The soft target a construct wraps to fit. A line with no break point in it (a long string literal) is left long. |
| `--no-trailing-comma` | off | Drops the trailing comma from multi-line records, sums, list literals and `exports` clauses. |

Nothing is written to `bynk.toml` — the override applies to that run only.
`--check` judges each file against the style the run asks for, so a project on a
non-default style still has a working CI gate:

```sh
bynkc fmt --check --indent spaces --indent-width 4 src/*.bynk
```

Format-on-save in the editor reads `[fmt]` from [`bynk.toml`](/docs/manifest/)
rather than these flags, so a project that sets a style there should pass the
matching flags to whatever script or CI job runs `fmt`.

## Related

- [Set up editor support](/docs/editor-and-tooling/editor-support/) for format-on-save.
- Reference: [`bynk-fmt`](/docs/tooling/bynk-fmt/).
