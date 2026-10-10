---
level: patch
changelog: "**A nested list type is valid TypeScript** ([#1898](https://github.com/accuser/bynk/issues/1898)). `List[List[Int]]` now emits `readonly (readonly number[])[]` instead of `readonly readonly number[][]`, which `tsc` rejected (TS1354); a function-typed element (`List[Int -> Int]`) is parenthesised the same way. Covers both declared types and the List/Query kernel lowerings' inline annotations."
---
