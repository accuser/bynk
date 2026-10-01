# 0421 — `==` is structural, and defined only on equality-supporting types

- **Status:** Accepted (v0.291)

**Context.** Before #1652, `==`/`!=` lowered to host `===` for every type except
`Bytes` (ADR 0142 D4). That is reference equality on records, sums, `Option`,
`Result`, `List` and value `Map`s: `Some(1) == Some(1)` was `false`, a nullary
variant decoded from JSON was unequal to its constant, and an invariant written in
the spec's own idiom (`status == Paid implies …`) could only ever see its
antecedent true while the loaded value happened to be the same object as the
constant. ADR 0142 recorded this as "a separate decision", never taken. The
Settled type-system spec (§2.3.5) already specified structural equality for sums,
recursively. The checker rejected `==` only on a *top-level* `Stream` or
`Connection`, so `Option[Connection[F]]`, `List[(Int) -> Int]`, a record with an
`Effect` field and `Box[(Int) -> Int]` were all accepted. `is` patterns already
compared structurally (`x.tag === "Paid"`), and test stubs matched arguments by
`JSON.stringify`, a third semantics.

**Decision.**
1. `==`/`!=` are structural. The emitter dispatches on the operand's type
   (`lower_bin_op`), the same operand-typed shape as division:
   - a statically primitive operand (a base type other than `Bytes`, `()`, or a
     refined/opaque type over one) keeps host `===`/`!==`;
   - `Bytes`, including an opaque type over it, uses `__bynkBytesEqual`. Before
     this ADR the opaque case fell through to `===`.
   - everything else, and an operand with no recorded type, uses the runtime's
     `__bynkEq`.
2. `__bynkEq` walks the in-memory representation, with no type information. It
   uses `===` at primitive leaves (so IEEE `NaN != NaN` and `-0 == 0` hold at any
   depth), content for `Uint8Array`, element-wise comparison for arrays, size then
   key and value for `Map`, own keys and values for plain objects (so a sum's `tag`
   decides first), and identity for class instances. A type-directed `equals_T`
   per type was rejected: Bynk generic functions are not monomorphised, so `==` on
   a type parameter has no concrete type to dispatch on without dictionary
   passing.
3. The checker defines equality-supporting types recursively, as §2.3.5 does. A
   function, `Effect`, `Query`, `Stream` or held `Connection` anywhere inside the
   operand's type (inside a container, a generic argument, or a record or sum
   field reached through its declaration) rejects `==`. A new code,
   `bynk.types.not_comparable`, covers computations; `Stream` and `Connection`
   keep their existing codes.
4. Test stub argument patterns match with `__bynkEq`, so the language has one
   equality. The separate `JSON.stringify`-based `__bynkDeepEqual` is removed:
   under it every `Map` compared equal and `NaN` equalled `NaN`.

Rejecting `==` on non-primitive types instead was considered and rejected. It
would reject the spec's own Settled examples and the invariant idiom, lose
`Some(x) == y` and record `expect`s, and still leave stub matching needing
structural equality.

**Known limits.** Both fail permissively, as every case did before this ADR: the
walker compares the offending part by identity.
- A type argument inferred at a call to a generic function is not checked. `==`
  on a type parameter `T` is accepted in the generic body, so
  `fn same[T](a: T, b: T) -> Bool { a == b }` called as `same(f, h)` on two
  functions compiles. Closing it needs equality bounds inferred per generic
  function (transitively through the functions it calls) and checked at each
  instantiation, which is a checker feature of its own (#1688).
- A record imported from another unit is not walked through its fields, because
  the checker's type map holds only the current unit's declarations.

**Consequences.** Supersedes ADR 0142's note that whole-record `==` "remains
reference equality". Programs comparing structured values now get the answer the
spec states; no correct program depended on reference results. `is_keyable`
(`Map` keys, `distinct`, `groupBy`, `@indexed`) is unchanged: structural keys are a
possible follow-on, not part of this decision. Proved at runtime by
`bynkc/tests/fixtures/positive/1652_structural_equality` (behavioural, 9 cases)
and the runtime package's `equality.test.ts`. Negative fixtures
`1652_eq_*` pin the recursive rule.
