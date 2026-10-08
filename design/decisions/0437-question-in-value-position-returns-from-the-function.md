# 0437 — A `?` in a value-position `if`, `match` or block returns from the enclosing function

- **Status:** Accepted (v0.307.1)

**Context.** The type system says `?` propagates an `Err` out of the enclosing function, and the checker validates every `?` against the function's return type, however deeply it is nested. The emitter lowered a value-position `if` (one not shaped as a ternary), `match`, or block to an arrow called in place, an IIFE. A `?` inside lowers to `if (r.tag === "Err") return r;`, which there returned from the arrow, so the `Err` became the expression's value. `let n = if a is Some(v) { r? + v } else { 0 }` produced `Ok(Err(e))` at runtime. `tsc --strict` rejects the result, but the bundler path does not type-check, so the program built. [[0178]] §E recorded the arrow's behaviour as its rule ("clears it for an IIFE so an embedding `?` behaves like a plain `?`"). That described the miscompile rather than intended semantics. T2.1 had already closed the same class for a ternary-shaped `if` (`hoist_if_as_statement`) and for short-circuit operands.

**Decision.** A value-position `if`, `match` or block whose body holds a `?` (outside any lambda, which is its own return scope) is emitted as a real statement:

```ts
let __r0: T;
__r0_out: {
  if (cond) { …; __r0 = a; break __r0_out; } else { __r0 = b; break __r0_out; }
}
```

It reuses the ordinary statement-position emitters, with a tail sink on the lowering context: each tail assigns the slot and breaks the label rather than returning. The label lets the `break` leave a `switch` (whose cases would otherwise fall through to its trailing `throw`) and any nested statement form. Function, lambda and arrow bodies reset the sink to `return`. The route is chosen structurally, before lowering. A form with no `?` keeps its arrow, so every other program's output is byte-identical. In statement form the `?` returns from the enclosing function, so the enclosing return type stays in force and a declared embedding converts the error. That supersedes [[0178]] §E's IIFE clause; an arrow without a `?` has nothing that reads the return type.

**Consequences.** An `await` inside such a form lands in the enclosing function's body, which is `async` whenever its body can `await`. Rejected: keeping the arrow and propagating an `Err` out through a runtime marker that the call site re-returns. That would be a second mechanism for a class the statement form already closes elsewhere, plus a new runtime type.
