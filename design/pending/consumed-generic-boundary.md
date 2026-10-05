---
level: patch
changelog: A context can now call a consumed context's service that takes or returns a generic type the other context exports, such as `Envelope[Int]`. The consumer's Workers boundary codec now names the type through the other context's namespace, where before it failed `tsc` with `TS2304`; generic sums get the same fix. Passing such a value as an argument no longer fails `bynk.boundary.structural_mismatch` against an identical type (#1736)
---
