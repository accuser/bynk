---
level: minor
changelog: "A `Matches` pattern must now be **safe to backtrack** (#1651). The `catastrophic_regex` guard caught only nested quantifiers (`(a+)+`). Ambiguous alternation (`(a|a)+`, `(\\d|\\d\\d)+`, `(a|aa)*`) and a bounded repeat inside a loop (`(a{1,2})+`, `(a{2,3})+`) passed, though each is exponential on request input: `(a|a)+` took 25.6 s on a 29-character path segment. The compiler now decides ambiguity on the pattern's automaton. Exponential ambiguity, and a pattern it cannot bound (a backreference inside an unbounded repeat, an unbounded repeat over a body that can match nothing, or a pattern too large to analyse), are `bynk.types.catastrophic_regex` errors. Polynomial ambiguity (`\\d*\\d*`) is the new `bynk.types.polynomial_regex` error, or the new `bynk.types.polynomial_regex_capped` **warning** when the refinement also has a `MaxLength` or `Length` predicate. Length predicates are now checked **before** `Matches` at runtime wherever they are written, so the bound caps the cost. That changes which failure a value failing both reports: the length one. `(foo|foobar)+` is unambiguous and still accepted. Of every `Matches` pattern in the repo, docs and examples, including the first-party `LocaleTag`, the only one newly rejected is the review document's own `(a|a)+`. Separately, `String.replace` inserts its replacement **literally** (#1650). It lowered to `replaceAll(from, to)` with a string replacement, which JS `$`-expands, so `\"aaa\".replace(\"a\", \"$&b\")` returned `\"ababab\"` rather than `\"$&b$&b$&b\"`, and `$1`, `` $` ``, `$'` and `$$` were rewritten too. When the replacement came from request data, the output depended on `$` sequences its author never wrote. It now lowers to a function replacer, `replaceAll(from, () => to)`, whose return value JS never expands. Programs that relied on the expansion lose it: Bynk's documented surface has no regex replace, so no `$1` group reference ever had a group to refer to. Proved at runtime by the behavioural fixture `1650_string_replace_literal`."
---

## ADR: regex-ambiguity-check
title: `Matches` patterns are admitted by an automaton ambiguity check
summary: Exponential and unbounded-polynomial ambiguity are compile errors; a length bound downgrades polynomial to a warning, and length predicates are checked first

**Context.** A refined `String`'s `.of`, its `is` test and its boundary codec run
the `Matches` pattern as `new RegExp("^(?:" + pat + ")$")`, with no flags, on
untrusted input. The workers route entry calls it on a raw path segment. V8's
`RegExp` backtracks. #724 rejected one exponential shape, nested unbounded
quantifiers, with a structural scan (`has_nested_unbounded_quantifier`). Its doc
comment named ambiguous alternation as "a deferred follow-up (#724)", and #724
closed with no issue tracking the rest. Settling research (track doc §3.3)
measured more: `(a|a)+`, `(\d|\d\d)+`, `(a|aa)*` and `(\w|\d)+` are
exponential, and so are `(a{1,2})+` and `(a{2,3})+`. A test pinned the latter as
safe ("finite *inner* bound cannot explode"). `\d*\d*` is polynomial (391 ms at
20,000 characters). `(foo|foobar)+`, which the guard's comment and #1651 listed
as exponential, is linear: {foo, foobar} is uniquely decodable. Runtime
mitigations are unavailable. workerd rejects V8's linear-engine flag `l`. A
WASM RE2 or Rust `regex` would cost most of the Worker bundle budget, and would
drop lookbehind and backreferences, which the corpus uses. A synchronous
`RegExp.test` cannot be timed out.

**Decision.**
1. `bynk-check/src/checker/regex_ambiguity.rs` decides a pattern's ambiguity
   after Weideman, van der Merwe, Berglund & Watson (CIAA 2016), the approach
   behind rxxr2 and recheck:
   - parse with ECMAScript no-flags semantics (UTF-16 code units, Annex B);
   - expand bounded repeats as nested optionals (`x{2,4}` is `xx(x(x)?)?`;
     the flat form invents ambiguity), over-approximating a count above 64 as
     unbounded;
   - build the Glushkov automaton.
2. **Exponential ambiguity** exists iff a strongly connected component of the
   product A×A, among the pairs reachable from the start pair, holds a diagonal
   and an off-diagonal pair. A transition the construction derives twice on a
   cycle is EDA too, which keeps `(a+)+` covered without the first pass.
   Exponential ambiguity is `bynk.types.catastrophic_regex`.
3. **Polynomial ambiguity** exists iff, for some states `p ≠ q`,
   `(p, p, q) ⇝ (p, q, q)` in A³. The search runs only from pairs where
   `(p, p) ⇝ (p, q) ⇝ (q, q)` in A×A, under a budget of 2,000,000 triples.
   Polynomial ambiguity is `bynk.types.polynomial_regex` (error), or
   `bynk.types.polynomial_regex_capped` (warning) when the same refinement has
   a `MaxLength` or `Length` predicate.
4. **Length predicates are checked first.** `bynk_syntax::ast::in_check_order`
   puts every `Matches` after every other predicate, keeping source order
   otherwise. `.of`, the boundary codec's inline checks, `is`, and the
   checker's compile-time literal check all use it. The track doc §3.3
   conditioned the downgrade on `MaxLength` *preceding* `Matches`. With the
   hoist that condition is moot, so the downgrade applies to any length
   predicate in the refinement. The contract fingerprint's canonical form sorts
   its own copy and is unchanged.
5. **Soundness obligations.** Each errs toward rejection:
   - assertions are read as matching anything;
   - a lookaround is read as matching anything where it stands, and its body is
     analysed as a pattern of its own;
   - a backreference is read as an optional copy of the group it names;
   - a backreference under an unbounded quantifier is rejected;
   - an unbounded quantifier over a nullable body is rejected;
   - a pattern over 2,000 positions, or over the triple budget, is rejected as
     too complex.

   These rejections share `catastrophic_regex`, with a message naming which
   one applied.
6. `has_nested_unbounded_quantifier` stays as a cheap first pass with its
   clearer message. The analysis runs only on patterns it accepts.

R1 was rejected: reject alternation under a loop unless its branches are
first-character disjoint. It is unsound, passing `(a{1,2})+` and `(a?a)+`. It is
over-strict, rejecting `(foo|foobar)+`. And it rejects the first-party
`LocaleTag`.

**Consequences.** Every exponential pattern the track doc measured is now a
compile error, and an accepted pattern's match time is linear in the input, or
polynomial under a length bound the runtime checks first. Compatibility, measured
over every `Matches` pattern in the repo, docs and examples: the only newly
rejected one is `(a|a)+` in the review document. `LocaleTag` analyses in about
8 ms in a debug build. The verdicts agree with V8:
- `([a-f]|[f-z])+` takes 6.4 s at 26 characters;
- `(a|ab|b)*` takes 529 ms at 22 characters;
- `\w+\d+` takes 385 ms at 20,000 characters;
- the linear `(foo|foobar)+`, disjoint-class and lookbehind patterns stay under
  2 ms on inputs of 20,000 characters or more.

A value failing both a length predicate and `Matches` now reports the length
failure. That message is the only observable effect of the hoist.

Proved by:
- `regex_ambiguity`'s unit tests: every shape in the track table, both
  directions, plus each soundness obligation;
- `redos_tests`, which now assert the analysis accepts every repo pattern and
  `LocaleTag`;
- nine `1651_regex_*` negative fixtures;
- the positive behavioural fixture `1651_regex_polynomial_capped`, which pins
  the warning and the length-first order in `.of` and `is`.
