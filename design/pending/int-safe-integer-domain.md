---
level: minor
changelog: "An `Int` is a JS **safe integer**, ±(2^53 − 1), at every entry point (#1657). An integer literal past 2^53 − 1 is `bynk.lex.integer_overflow`; it was accepted up to the 64-bit range and rounded, so `9007199254740993 == 9007199254740992`. The JSON boundary rejects a number that isn't a safe integer (`1e300`) on decode, and faults instead of writing `null` when encoding an `Int` that isn't one, so a state commit of such a value is refused with nothing persisted. `Int.parse` and `Float.parse` take a strict full-string decimal grammar: `\" 7 \"`, `\"0x10\"` and `\"0b11\"` are `None`, and for `Int` so are `\"1e3\"` and `\"5.\"`; a leading `+` is accepted, and `\"-0\"` parses as `0`. `Int` division by zero is a runtime fault instead of `Infinity`/`NaN`, and `round`/`floor`/`ceil`/`truncate` fault when the result isn't a safe integer. A WebSocket `on open` parameter must be constructible from `String` (`bynk.service.websocket_param_not_stringy`); an `Int` there was a query-string value cast `as number`, so `room + 1` gave `\"51\"`. Overflow in `+ - *` stays documented imprecision. An agent whose stored state holds an `Int` outside the range now faults with `RehydrationViolation` on load, and must have that value corrected or its storage cleared once."
---

## ADR: int-safe-integer-domain
title: `Int` is the JS safe-integer domain, enforced at every entry
summary: Literals, the wire (both directions), Int.parse, division by zero and Float→Int conversions all hold an Int to ±(2^53 − 1)

**Context.** No document defined what an `Int` is, and the language's entry points
disagreed:
- the lexer accepted any `i64`, so `9223372036854775807` was emitted verbatim and
  rounded;
- the boundary decoders accepted any `Number.isInteger`, including `1e300`;
- `Int.parse` ran `trim()` then `Number()`, so `"0x10"`, `"1e3"` and `" 7 "` all
  parsed, contradicting the spec's "full-string" rule.

Settling (`design/tracks/runtime-semantics.md` §3.4) found the type unsound, not
only imprecise:
- `5 / 0` produced `Infinity` as an `Int`, which `Json.encode` wrote as `null`
  and the consumer then rejected;
- `round`/`floor`/`ceil`/`truncate` turned a non-finite `Float` into an `Int`;
- a WebSocket `on open (room: Int)` parameter reached the handler as an unparsed
  query-string value cast `as number`.

ADR 0048 already called `isSafeInteger` "the honest bound", and ADR 0319 noted the
i64-versus-2^53 gap without deciding it. The alternative, i64 via `bigint`,
breaks `JSON.stringify`, the wire format and every consumer, and changes
arithmetic, `length()` and `Duration` interop.

**Decision.**
1. **Domain (A).** An `Int` is a JS safe integer, −(2^53 − 1) to 2^53 − 1.
2. **Literals.** The lexer rejects an integer literal whose magnitude exceeds
   2^53 − 1. That covers `InRange` bounds and every other literal position. The
   existing `bynk.lex.integer_overflow` is reused with a new message and
   summary; the concept is unchanged, only the domain. This supersedes §3.4's
   proposed new code `integer_out_of_safe_range`.
3. **Decode.** Every boundary integer guard uses `Number.isSafeInteger`: the
   field and named-type `Integral` guard (shared with `Instant`), the inline
   base codec (shared with `Duration`/`Instant`), the refined `.of` guard and
   the refined `is` check. Errors now say "safe integer".
4. **Encode (C).** Serialising an `Int` that isn't a safe integer throws. It
   uses the same self-contained guard a non-finite `Float` uses
   (`guarded_number`). `Duration`/`Instant` encodes are not guarded: no
   in-language operation produces a non-integral or out-of-range one short of
   `Int` arithmetic that is itself now guarded at its boundaries.
5. **`Int.parse` (B)** requires `^[+-]?[0-9]+$`, then a safe integer, and
   normalises `-0` to `0`. The issue proposed `^-?[0-9]+$`; §3.4's leading `+`
   is adopted. **`Float.parse`** requires
   `^[+-]?([0-9]+(\.[0-9]*)?|\.[0-9]+)([eE][+-]?[0-9]+)?$`, then a finite
   value; it still accepts `"5."` and `".5"`.
6. **`Int` division by zero** is a runtime fault (`Error("Int division by
   zero")`), with operands still evaluated left to right. Bynk has no `%`, so
   division is the only such case.
7. **Float→`Int` conversions** (the decision carried into S8): `round`,
   `floor`, `ceil` and `truncate` fault when the result isn't a safe integer,
   which covers a non-finite input and one past ±2^53 in one check. This is
   option (b), for consistency with division. Option (a), returning
   `Option[Int]`, was rejected: it changes the signatures of four kernel
   methods, a language change for an edge case.

   Both traps are runtime helpers, `__bynkIntDiv(l, r)` and
   `__bynkToInt(Math.<op>(f), "<op>")`, not inline guards. An inline guard put
   a never-taken `throw` branch on every division line, which V8 block coverage
   reports, so `bynkc test --coverage` marked those lines uncovered. Runtime
   lines are never attributed to `.bynk` source.
8. **WebSocket `on open` parameters** must be constructible from `String`, as
   HTTP path parameters are, under a new code,
   `bynk.service.websocket_param_not_stringy`. It sits in the `service`
   category the other WebSocket rules use; reusing
   `bynk.http.path_param_not_stringy` would leave that code's documented
   summary wrong. Inbound and close route values are a prefix of these, so
   they inherit the rule.
9. **Not decided here:** overflow in `Int` `+ - *` stays documented imprecision,
   as ADR 0042 has it ("arithmetic host-defined, boundaries guarded"). The
   boundaries are the guarantee: an out-of-range value can be neither accepted
   from outside nor written out undetected.

**Consequences.** `Int` has one stated domain, and the spec's `Int.parse` wording
is now true. This supersedes ADR 0319's open note on the i64 gap and closes the
"Int precision" roadmap item.
- **A program with a literal past 2^53** no longer compiles. Such programs were
  already silently wrong.
- **`Int.parse`** rejects inputs it used to accept (hex, exponents, padding).
- **Stored state:** an agent whose stored state holds an out-of-range `Int` now
  faults with `RehydrationViolation` on load, which needs a one-time fix of the
  value or a storage clear. The changelog says so. This is the same posture
  #1687 took for a non-finite `Float` at commit.
- **Goldens:** every generated module with an `Int` at a boundary moved (about
  185 files: `isSafeInteger`, the encode guard, and the new trap IIFEs).

Proved by:
- the behavioural fixture `1657_int_domain`: the `Int.parse`/`Float.parse`
  tables and `Json.decode` at the range edge. Its two grammar cases fail on the
  previous lowering.
- `bynkc/tests/store_behaviour.rs::int_stays_a_safe_integer_at_runtime`:
  division, each conversion, `Json.encode`, a refused state commit, and
  rehydration of a stored `1e300`. It fails on `main` at the first trap.
- negative fixtures: `1657_int_literal_out_of_safe_range`,
  `1657_inrange_bound_out_of_safe_range` and
  `1657_websocket_open_param_int`;
- the lexer's unit tests.
