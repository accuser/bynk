# Bynk compiler pipeline — architecture and code-quality review

**Reviewed at:** v0.237.1, 27 July 2026
**Scope:** the full pipeline (`bynk-syntax` → `bynk-check` → `bynk-emit` → `bynkc`/`bynk-driver`/`bynk`), weighted towards the back end, plus the cross-cutting concerns that span it. `bynk-lsp` was only partly available and is covered indirectly through `bynk-ide`.
**Emphasis requested:** architecture and alternative approaches first; code quality and maintainability second.

---

## How this review was produced, and how much to trust it

Twelve independent reviewers each took one crate or one cross-cutting dimension and read the source directly. Every finding was then handed to a second reviewer whose brief was to refute it: open each cited line, check the numbers, look for a doc comment or design note that already justifies the choice, and reject anything that reads as generic compiler advice with line numbers bolted on. Seventy-two findings survived that pass — forty-seven confirmed outright, fourteen confirmed with corrections, eleven verified separately after two verifier agents failed on a transient API error.

Two caveats on the evidence. The checkout under review contained the `src/` trees and a selection of design documents, but not the `tests/` directories — so no finding here rests on the absence of a test file, and where test coverage is discussed it is inferred from in-file `#[cfg(test)]` modules and from what the source comments say about the fixture suite. Second, `design/decisions/` was largely unavailable, so where a comment cites an ADR by number the reviewers engaged with what the comment says the ADR decided rather than with the ADR itself. Line numbers throughout have been checked against the tree at the reviewed commit; where the original reviewer's citation drifted, the corrected line is given.

I read `design/archive/bynk-refactor-proposal-queue.md` before starting and fed it to every reviewer as a do-not-repeat list. That June 2026 review's items 1, 2, 4, 5, 6 and 8 have landed — `project.rs` is split, `checker.rs` is split, `compile_project` takes a single `CompileOptions`, `UnitInfo` exists, `builtin_names` exists. Nothing below re-proposes them. Item 9 (a `CodeWriter`) and item 12 (declaration cloning) recur here, but on different evidence and with a different recommendation than last time.

---

## Headline

This is a well-run codebase. The decomposition track worked: the crate graph is a real DAG, the phase structure is legible, and the density of *reasoned* comments — comments that say why a thing is the way it is, and what was rejected — is higher than in most production compilers. The discipline of pairing a registry with a test that asserts the registry matches the source (`diagnostics.rs`, `keywords.rs`) is a genuinely good pattern and it recurs.

The problems are not the ones a size-based reading would predict. The 5,000-line files are mostly fine; they are long because the language is large, and the splits that were needed have happened. What has gone wrong is subtler and more consequential: **the pipeline has grown three or four load-bearing invariants that are enforced by comment rather than by type, and every one of them has already been violated at least once in shipped code.** The pattern repeats with enough regularity that it is worth naming as the review's central claim.

In each case the shape is identical. Some fact — "this sub-expression hoists no statements", "no two typed AST nodes share a span", "`local_type_names` means locally declared, not merged", "this list of kernel methods matches that one" — is true, is important, is written down in a doc comment, and is not represented anywhere the compiler can check it. The codebase then grows a new site that needs the fact, the new site gets it wrong, and the failure is silent: wrong TypeScript, a missing completion, a disabled security check. Several of these are already documented in the source as past regressions, which is the strongest possible evidence that the mechanism, not the individual bug, is the thing to fix.

The second theme is smaller but pervasive: **the crate names do not describe the layering.** `bynk-check` does not check contexts. `bynk-emit` is where the language's semantic analysis lives. `bynk-ide` links the whole TypeScript back end. None of this is hidden — the doc comments are candid — but it means the seams that the decomposition track created are not the seams that exist, and a contributor reading the crate docs will be misled about where to make a change.

Nothing here suggests remediation. This is paydown, and most of the highest-value items are small.

---

## Part 1 — The architecture

### 1.1 The emitter has an IR; it just isn't typed

`emitter/lower.rs` is presented as lowering, and the crate documentation is careful to say that Bynk goes from typed AST straight to TypeScript text with no intermediate representation. That is not quite what the code does. There *is* an IR, and it is the pair `(String, &mut Vec<String>)`: `lower_expr(e, stmts, cx) -> String` returns an expression's text and appends any statements that must run before it to a vector the caller supplies. Twenty-seven signatures in `lower.rs` carry that `stmts: &mut Vec<String>` parameter.

The problem is that nothing in the type forces a caller to consume what it was given, and three callers do not. `lower_if`'s ternary path builds a throwaway vector, asserts it is empty in debug builds, and drops it (`lower.rs:3393-3399`); `lower_tail_expr` repeats the same code verbatim (`lower.rs:293-298`). The gate protecting those `debug_assert!`s is `simple_expr` (`lower.rs:3532`), which correctly excludes `Question` and `Match` but returns `simple_expr(value)` for `ExprKind::Is` at line 3556 — while `is_receiver_ref_forced` (`emitter.rs:3087`) unconditionally pushes `const __rN = <recv>;` for a refined `is` check, even on a bare identifier. So the classifier says "hoists nothing" for a construct that always hoists. In a debug build the compiler panics on valid user code; in release it emits a reference to a variable that was never declared.

The third case is worse because there is no assertion at all. `lower_match_as_iife` (`lower.rs:4169`) lowers the discriminant into a local vector and, when it is non-empty, wraps those statements in a *fresh synchronous arrow* before returning the inner IIFE. A `?` in the discriminant contributes `if (__r0.tag === "Err") return __r0;`, so for `let x = match risky()? { … }` the error early-return exits the wrapper arrow instead of the enclosing handler, and the match expression silently evaluates to the `Err` object. That is a miscompile of ordinary code with no diagnostic and no assertion.

The same side channel is behind two further defects. `lower_and_with_is` (`lower.rs:3759`) flattens its right-hand side's hoisted statements into a string and splices them where an expression is required (`lower.rs:3787`, `3797`) — and note that the `bindings.is_empty()` branch at 3786 is the *common* case, not an edge one, so `state is Held(_) && …` takes it. And `lower_bin_op`'s general path (`lower.rs:3818-3819`) lowers both operands into the same caller vector, so a hoist from the right-hand operand is emitted unconditionally before the operator — which defeats short-circuit evaluation. `design/bynk-type-system.md:1671-1678` is explicit that this is not merely an optimisation: "This is both a performance property and a safety property … Developers can rely on this." The verifier corrected the scope here usefully: the spec's own worked example (`state is Held(_) implies <expensive query>`) routes through `lower_and_with_is` and *does* preserve short-circuiting, so what breaks on that shape is syntax rather than evaluation order. The evaluation-order violation applies to `||` in every form (`lower_and_with_is` is never consulted for `Or`) and to `&&`/`implies` whose antecedent contains no `is`.

The fix is one change and it closes all of it. Make `lower_expr` return `Lowered { pre: Vec<String>, expr: String }` instead of taking a sink. Every caller then has to say what it does with `pre`; the two ternary sites become compile errors until they either bail to the IIFE form or hoist into the enclosing block, and `lower_and_with_is` cannot flatten statements into a string because `Lowered` is not a `String`. Short-circuit correctness becomes a two-line rule in `lower_bin_op`: for `And`/`Or`/`Implies`, if the right-hand side's `pre` is non-empty, wrap it in the IIFE form. The threading is mechanical across roughly ninety functions, most of which just pass the value along, and it subsumes both `debug_assert!`s.

This is the same move the codebase already made once, deliberately, and wrote down why. `runtime_use.rs:35-52` argues at length against deciding things by scanning generated text — "It over-matches … Worse, it under-matches" — and `RuntimeUse` exists to make import decisions travel in a type rather than in a convention. The recommendation here is that argument applied to the other side channel in the same file. The prior art for the specific shape is tsc's transformer API, which pairs an expression result with an explicit `context.hoistVariableDeclaration` call, and Babel's `path.scope.push`: in both, the hoist is a named operation on a scope, never an implicit out-parameter.

There is a second instance of the text-scanning pattern that `RuntimeUse` was built to retire, still live and now making a semantic decision. `maybe_async_iife` (`lower.rs:4214`) determines whether a value-position `match` needs an async arrow by asking `if !iife.contains("await ")`. It over-matches on any arm containing one of the iterator terminals, which emit self-contained async IIFEs as ordinary `Effect` values (`lower.rs:2756` for `forEach`, and four siblings) — so `match x { A => items.forEach(f), B => Effect.pure(unit) }` flips the outer arrow to async, awaits a value the arm deliberately returned unawaited, and emits a bare `await` that is a syntax error in a synchronous enclosing scope. It also is not applied at all by `lower_if`'s IIFE (`lower.rs:3402-3444`), which builds its own arrow and never calls it — so `let r = if c { let y <- fetch(); y + 1 } else { 0 }` puts an `await` inside a synchronous arrow. The same bug class was found and patched for `match` and left unpatched for `if`, which is the giveaway that the fix was a local text patch rather than a property of the lowering. Set a flag on `LowerCtx` at the two sites that actually emit `await`, read-and-reset it around each IIFE construction, and delete the scan.

### 1.2 `bynk-check` does not check contexts

`checker::check_record` (`checker.rs:283-306`) matches exactly two item kinds: `Type` and `Fn`. The resolver is explicit about the rest (`resolver.rs:222-232`): capabilities, providers, services, agents, actors and messages "go through the context-level v0.5 path in project.rs". So everything that makes Bynk *Bynk* — capability and provider signatures, HTTP/cron/queue handler shape, actor `by` contracts, agent `store` fields, `@indexed` hygiene, CORS and rate-limit policy — is checked in `bynk-emit/src/project/validate.rs`. Counting diagnostic codes: 207 distinct `bynk.*` codes originate in `bynk-check`, 190 in `bynk-emit`, 110 of them in `validate.rs` alone.

This is not merely a naming complaint, for three reasons.

First, `validate.rs` reaches back across the crate boundary to drive the checker, and to do that it reconstructs a `ResolvedCommons` by hand. There are **six** hand-rolled constructions of that type in `bynk-emit` (`project.rs:2962`, `validate.rs:930`, `validate.rs:3111`, `validate.rs:3199`, `tests_emit.rs:764`, `tests_emit.rs:3173`) against one legitimate construction inside the resolver (`resolver.rs:428`). `ResolvedCommons` has ten public fields whose invariants the resolver establishes; each of those six sites independently decides what they mean. They already disagree. `validate.rs:920-935` carries a hard-won comment explaining that `local_type_names` must be the *pre-merge* local table, because "reusing `typed.types` here made every consumed/used type read as 'local', silently over-widening `.raw`/`.unsafe()` access on a consumed opaque type" — and then, 2,180 lines later, `check_agent_decls` does precisely that (`validate.rs:3108-3116`, and again at `3195-3204`). Three checker gates are therefore off inside agent handler bodies today: `.raw` on an opaque type, `T.unsafe(…)`, and owner-only event emission — the last of which its own comment calls "the primary boundary guarantee the threat model names". The event case is confirmed reachable, not theoretical: `symbols.rs:301-309` registers each `event` declaration into `types`, so a foreign event arriving through `consumes` lands in `typed.types` and reads as local. Agents are the main event emitters. Meanwhile a third site, `tests_emit.rs:769`, sets `local_type_names: HashSet::new()` — over-narrowing in the opposite direction. Six sites, three mutually incompatible readings of one documented field.

Second, the checker's working context is public API. `pub struct Ctx<'a>` (`checker.rs:1281`) has 24 public fields — a memo cache, flow flags, five parallel `store_*` maps — and three sites in `bynk-emit` construct it as a struct literal and call `type_of_block` directly (`tests_emit.rs:794`, `2018`, `2970`). They already disagree with each other: one passes `CapabilityCtx::default()`, another spells out a fully populated capability map. Adding a field to the checker's context — the natural move every time a new store kind lands, and five have accreted this way — is a breaking change to another crate that surfaces as three compile errors inside a 6,000-line file. `HandlerBodyCheck` and `CheckSinks` were built in #522 as the proper entry point, and these three sites bypass them, so the abstraction earns nothing at exactly the places that most need it.

Third, anything that wants Bynk diagnostics must link the emitter. `bynk-ide/src/lib.rs:25-31` states that `bynk-lsp` "deliberately does not depend on `bynk-emit`", and `bynk-lsp/Cargo.toml:45` says the language server "links the analysis libraries directly, not the whole `bynkc` compiler crate". Both are true only at the manifest level: `bynk-ide` lists `bynk-emit`, so the LSP compiles and links roughly 24,850 lines of pure emission code — `emitter.rs`, `emitter/*` and `tests_emit.rs`, about 63% of the crate — that it never executes, and pulls it through thin LTO in the release binary. Every edit to `lower.rs` rebuilds and relinks `bynk-lsp`, `bynk-ide`, `bynk-strip`, `bynk-wasm`, `bynk` and `bynkc`.

The strongest argument for doing something about this is that the author has already made exactly this call once, for exactly this reason. `bynkc/Cargo.toml:32-35` explains that strip-only TS→JS lives in its own crate "so oxc stays out of `bynk-emit` and the LSP, which need neither stripping nor JS output". The emitter is the larger instance of the same case.

The full move — lifting the semantic layer down into `bynk-check` — is a very large change and the verifier found a real obstacle the original reviewer missed: `validate.rs` depends on `crate::emitter` for more than string escaping. It uses `emitter::placeholder_names`, `template_format_kinds`, `icu_dispatch_placeholders` and `parse_icu_placeholder` (`validate.rs:371-396`, `554-555`), and `emitter::websocket::analyse_open_shape` (`validate.rs:1348`). So the move drags `icu.rs` (1,012 lines) and the WebSocket shape analysis with it.

That suggests a sequence rather than a single decision. Start with the pieces that are misfiled rather than entangled: `emitter/icu.rs` is a dependency-free ICU MessageFormat parser whose own doc comment names its consumer as "the checker (`bynk-emit/src/project/validate.rs`)" — the author already thinks of `validate.rs` as checker-side code. Move it and `websocket::analyse_open_shape` down to `bynk-check`. Then give `ResolvedCommons` a constructor that takes the local table and the merged table as separate arguments and computes `local_type_names` itself, with private fields, so the class of bug above becomes unrepresentable — that removes five hand-rolls and is the cheap 90% of the whole finding. Then add a `check_body` entry point beside `check_handler_body` (roughly fifteen lines wrapping `checker.rs:511`, since `HandlerBodyCheck::new` and `CheckSinks` already exist) and demote `Ctx` to `pub(crate)`. Whether the larger relocation is ever worth it can be decided later; those three steps are worth doing regardless of the answer.

Two immediate fixes are independent of all of that and should not wait: derive `local_names_for_handler` from `table.types.keys()` at `validate.rs:3109` and `:3197`, and add a fixture per gated diagnostic exercising it from inside an agent handler — there is evidently none, since all three are currently unreachable there.

### 1.3 `Span` is node identity

No AST node carries an identity. `Expr` is `{ kind, span }`, and the entire checker→emitter type channel is `TypedCommons.expr_types: HashMap<Span, Ty>` (`checker.rs:241`), where `Span` is a bare `{ start: usize, end: usize }` byte range. Expressions and blocks share one keyspace. Thirty-plus emitter sites read it, and the miss branch is not an error — it is `_ => "unknown".to_string()` (`lower.rs:3155`, `3189`, `3245` and neighbours), which interpolates the literal text `unknown` into the emitted TypeScript.

The invariant that keeps this working — "within one file, no two AST nodes that need a type share a span" — is unstated and unenforced, and has been violated twice. Once in shipped code: `checker.rs:2500-2510` carries a guard whose comment describes bug #844, where a synthetic single-expression block had `block.span == block.tail.span` and clobbered the tail expression's more specific entry. The fix was `if block.span != block.tail.span` — a point patch on the identity model, not a fix to it. And once today: the else-less `if` synthesises an else-block and a `UnitLit` tail both carrying `span: then_block.span` (`expressions.rs:1549-1557`), so three distinct nodes share one key. The verifier established that this second case is not currently an emitted-TypeScript bug, because no emitter read keys off a `Block` span; the live consequence is that `ExprTypeSink` ships the clobbered entry to the LSP, so hover over an else-less `if`'s then-branch reports `()` where the branch is `Effect[()]`. That makes it a confirmed second violation rather than a shipped miscompile, which is the finding's actual point. There is also a third span-keyed side table nobody mentioned: `is_binding_cache: HashMap<Span, Vec<(String, Ty)>>` (`checker.rs:1312`) memoises refinement bindings by condition span, so a collision serves stale bindings too.

A full `NodeId` retrofit — rustc's `HirId` plus indexed `TypeckResults` — is the right end state and is a large change across a 2,806-line AST and three consumer crates. Three cheaper steps get most of the value and are the migration scaffolding if the retrofit ever happens. Newtype the key as `ExprKey(Span)` so every site reads as identity rather than location; this is a type-alias change plus about eleven signatures that currently thread a bare `&mut HashMap<Span, Ty>`. Give blocks their own map. Then add a debug-only uniqueness check at the end of `check_record` that walks the unit with the existing total child iterator `ast::expr_children` and asserts no two typed nodes share a key — that would have caught #844 on the day it was introduced and catches the else-less-`if` aliasing today. Finally, replace `_ => "unknown".to_string()` with a loud internal error, so a miss fails the build rather than quietly degrading the product's headline claim.

### 1.4 Wide mutable contexts configured by assignment

Three structs in the pipeline are wide, mutable, and configured after construction rather than at it.

`LowerCtx` (`emitter.rs:2565-2820`) has **48 fields**. `LowerCtx::new` supplies three and defaults forty-five; every caller then mutates. There are fifteen construction sites (twelve production, three in tests) and roughly seventy `cx.<field> = …` assignment lines, and each site's list is bespoke: seven fields for a service handler, eight for a provider op, twelve for an agent handler, nine for a WebSocket lifecycle — and exactly one for an invariant predicate and one for a transition predicate. Adding a lowering mode means finding all fifteen sites and guessing which of the forty-five defaults are wrong for the new one, and "not assigned" and "deliberately default" are indistinguishable in the source.

`Ctx` (`checker.rs:1281`) has 24 fields including five parallel `store_*` maps, initialised to five empty maps at each of nine construction sites. A store field name lives in exactly one of the five, but nothing expresses that; dispatch is five sequential `else if` clauses.

`EmitProjectCtx` (`project.rs:4704-4804`) has 28 fields, five of which have **no readers anywhere in the repository** — `local_files`, `commons_dir`, `exports_local`, `is_consumed_by_others`, `boundary_type_owners`. Every occurrence is a construction site. Two of them (`is_consumed_by_others`, `boundary_type_owners`) are per-file project scans, computed and discarded once per emitted file. Deleting all five, plus `compute_boundary_type_owners` and the `BoundaryOwner` enum, removes about sixty lines and two per-file project scans and cannot change the emitted TypeScript, because nothing reads them.

For `LowerCtx` the shape to reach for is already visible in the data: split along the three lifetimes that exist. A `ModuleCtx` borrowed once per emitted module (commons, cross-context info, runtime use, target, local agents, rebrand info) makes `target` structurally impossible to forget; a `BodyMode` enum with per-variant payloads (`ServiceHandler { … }`, `AgentHandler { store } `, `Invariant { var, fields }`, `Transition { old, new }`, `TestCase { … }`) turns "which fields apply here" from tribal knowledge into an exhaustive match. That is a large change and should probably wait behind the `Lowered` return type from §1.1, which touches the same functions.

For `Ctx`, the five `store_*` maps want to be one `HashMap<String, StoreField>` over an enum. That is the `UnitInfo` move (queue item 6, already landed for units) recurring one layer down, and it collapses the five-clause dispatch chain into one lookup and a `match` — which also makes the "not a store field" fallthrough a single explicit arm. One caution from the verifier: `check_store_map_op` is *not* a verbatim superset of `check_store_cache_op`; the map version carries a held-resource rejection (`bynk.held.unsupported_map_op`) that the cache version correctly lacks. That asymmetry is deliberate, so a naive merge would be wrong.

For `EmitProjectCtx`, just delete the dead fields and make the struct `pub(crate)` — nothing outside the crate constructs it.

### 1.5 The drift class

Twelve separate findings are instances of one problem: a fact is written out in more than one place, and the places have no relationship the compiler can see. A grep for the phrases the codebase uses to pin these by convention — "in sync", "mirrors", "parity", "must match" — returns 233 hits across the Rust sources. Four of the duplications have already drifted.

**The kernel-method vocabulary exists in six copies, and `LIST_METHODS` has fallen behind.** The typing dispatch `check_list_kernel_method` accepts `join`, `joinOn`, `leftJoin` and `groupBy` on a `List` (`kernels.rs:714-723`), and the not-found message at `kernels.rs:741` advertises all four. `LIST_METHODS` (`kernel_methods.rs:32`) lists 28 names and contains none of them. `methods_for` is the IDE's only view of the value vocabulary, so `.`-member completion and signature help silently omit four methods the compiler accepts — and they are the newest ones (v0.94), which is when it matters. `QUERY_METHODS` does list them, so the List table fell behind its own sibling for the same feature. There is a sixth copy, `is_query_op` (`kernels.rs:762`), which lists 29 names including `collect`, which appears in neither. The drift test is documented as one-directional: it catches phantom entries, not missing ones. Making it bidirectional is an hour's work and would have caught this — generate the `method_not_found` message text from `LIST_METHODS` instead of hard-coding it, and the two cannot diverge. The larger proposal in the original finding (a machine-readable `Shape` for each entry, mechanising perhaps twenty of thirty-two arms) is defensible but the verifier advised dropping it, and I agree: the bidirectional test is the whole win at a fraction of the cost.

**Three copy-pasted runtime-import probes have diverged on three axes.** `file_mentions_json_error` (`emitter.rs:817`), `file_mentions_http_result` (`:866`) and `file_mentions_connection` (`:907`) are structurally identical functions. The first has no `Capability` arm, so a capability op returning `Result[Order, JsonError]` in a Json-call-free unit emits an interface naming `JsonError` under a header that omits it. The first is exhaustive over `TypeRef`; the other two end `_ => false`. And most tellingly, the `TypeRef::App` recursion added under ADR 0183 landed in `file_mentions_json_error` *only* — so `fn page() -> Paginated[HttpResult[Order]]` does not trigger the `HttpResult` import. A fix that landed in one of three copies and missed the others is exactly the failure mode `json.rs:11` warns about in its own comment.

**A second, syntactic type equality returns `false` for half of `TypeRef`.** `type_refs_match` (`validate.rs:3334`) covers nine of eighteen `TypeRef` variants and ends `_ => false`, silently swallowing `List`, `Map`, `Query`, `Stream`, `Connection`, `QueueResult`, `History`, `Fn` and `App`. It is the sole provider↔capability signature test (`project.rs:2228`, `:2243`). So a `provides Repo` whose op signature is byte-identical to the capability's gets `bynk.provider.signature_mismatch` printing two identical types — the worst possible diagnostic, because the user cannot act on it. This is not exotic: `design/bynk-type-system.md:464` uses `query(filter) -> Effect[List[ActivitySummary]]` as *the* worked capability example. There is also a third consumer beyond diagnostics — `emit.rs:3830-3843` uses it to decide whether a WebSocket `on message` parameter is filled with the decoded frame or a positional route value, where a false negative is silently wrong emitted TypeScript. Thirty lines above the definition, `build_capability_op_info` already does the right thing: resolve both sides through `checker::resolve_type_ref_in` into a `Ty` and compare with `==`. Delete `type_refs_match` and do that. If a purely syntactic comparison is genuinely wanted somewhere, make it exhaustive so the next variant is a compile error.

**The `serialise` dispatch has one surviving parallel copy.** `workers_entry.rs:1509-1515` records that this bug class was found and fixed twice — "Both of these used to be a *parallel* dispatch that shadowed `serialisation.rs`'s — and drifted from it." `serialise_call` and `deserialise_call` were fixed. `http_value_serialiser` (`workers_entry.rs:1473`) was not: it collapses every base type to `(v: any) => v as JsonValue`, so a root `Float` payload loses its non-finite guard (`JSON.stringify(NaN)` produces `null` with a 200) and a root `Bytes` payload ships the raw in-language value where the codec declares a base64 `string`. The `any` swallows the mismatch at `tsc --strict`, so the crate's stated backstop cannot catch it either. The fix is small and symmetric: `deserialise_ref_via` already exists at `serialisation.rs:1185`; add the point-free `serialise_ref_via` beside it and delete `http_value_serialiser`. While there, make `serialisation::inner_ts_name` `pub(crate)` and delete its verbatim copy at `workers_entry.rs:1529`.

Beyond the four that have drifted, three more are pinned only by prose and worth knowing about. The refinement predicate → runtime check mapping is written twice (`emit.rs:211` and `serialisation.rs:581`), and while adding a `PredKind` variant is a compile error at both, *amending* one drifts silently — and they already differ in substance: the inline version anchors its `Matches` regex with `^(?:…)$` and the other does not, while both emit the identical message `must match /{pat}/`. "Value-keyable" (ADR 0110 D5) is implemented four times across two crates (`resolver.rs:1030`, `kernels.rs:90`, `validate.rs:2580`, and a `Ty`-shaped fourth at `lower.rs:4026` that decides `Number(raw)` versus pass-through — that one miscompiles rather than merely rejecting if the set widens). And `index.ts` is spelled independently at three sites in two crates with no shared constant, where `bynk-strip` rewrites the wrangler `main` key by literal substring match (`bynk-strip/src/lib.rs:139`) against a string `wrangler.rs:33` happens to emit — reformat that line's spacing and `--emit js` silently produces a `wrangler.toml` pointing at a file that no longer exists.

Finally, three hand-rolled `Expr` recursions still shadow `ast::expr_children`, the canonical total child iterator, and one of them keeps the `_ => {}` wildcard that `linearity.rs:239-263` migrated away from after being bitten by it ("the old `_ => {}` silently skipped them all"). At least twelve call sites have already adopted `expr_children`; `walk_expr_for_index_filters` (`validate.rs:2770-2858`), `emitter::walk_exprs` (`emitter.rs:521`) and `walk_expr_for_constraints` have not. Worse, three *partial* walks sit directly beneath the exhaustive `walk_exprs` in the same file and answer "does this body do X?" — and `block_writes_state` (`emitter.rs:732`) has no `Lambda` arm, so `items.forEach((x) => orders.put(x.id, x))` returns false, `writes_state` is false, and the emitter omits the `await this.commitState(__state)` call. That is a silently lost durable write in idiomatic, well-typed Bynk.

---

## Part 2 — Things that are wrong today

The findings above are architectural, but several have concrete consequences that stand on their own and are cheap to fix independently. Collected here for triage, roughly in order of how much they cost a user.

A durable write is silently dropped when a store mutation happens inside a lambda (`emitter.rs:732`), because the "does this body write state?" probe does not descend into `Lambda`. Add the arm, or better, replace all three partial probes with `expr_children`-based walks.

A local variable that shares a name with a store field silently compiles to a storage access (`checker.rs:2829-2882`). Two of the three provenance-dispatch sites correctly guard with `ctx.lookup(name).is_none()`; the five-arm MethodCall chain does not, even though the HttpResult arm eleven lines below it does. The emitter independently makes the same precedence mistake (`lower.rs:1162`, and the ordering at `lower.rs:3586-3620`), so the two agree on the wrong answer and no consistency check can catch it. Five tokens fix the checker; the emitter needs the local check moved ahead of the store checks. Better still, since a store field shadowed by a local is almost certainly a source bug, reject it at the binding site and stop having to keep six dispatch sites in agreement forever.

An expression-bodied lambda does not rebind `return_ty`, so a `?` inside it uses the *enclosing function's* error embedding (`lower.rs:4094-4113`). The sibling `Block` arm fifteen lines above does this correctly and carries an explicit ADR 0178 comment explaining why it must. Hoisting the save/restore out to wrap the whole `match` is a five-line move that is byte-identical for the Block arm.

`bynkc test` with an integration suite silently rebuilds every non-context module with the wrong compile options (`main.rs:196`). The second compile calls `project_options(&input).target(Workers)` rather than deriving from the options `run_test` carefully built, so it loses `contracts: true` and `import_ext: Ts` — precisely the two fields `run_test` bothered to set. The consequence is that contract call-site guards are stripped from the modules the suite runs against, and `--inspect` emits `.js` specifiers over a `.ts`-only tree and dies with `ERR_MODULE_NOT_FOUND`. Adding `#[derive(Clone)]` to `CompileOptions` and writing `options.clone().target(Workers)` is a one-line fix.

The linearity pass loses a held value's disposal obligation when a binding name is reused in the same block (`linearity.rs:141-157`). The two other binding-introduction paths in the same file explicitly save and restore any shadowed entry; the `let` path was missed. Two sequential `let c <- conns.get(…)` statements followed by one `close()` leaks the first connection with no diagnostic. About six lines.

Warnings print the word "Error". `Severity::for_error` classifies six categories as warnings and the short renderer, the JSON path and the LSP all honour it, but `report_with_config` hardcodes `ReportKind::Error` at `error.rs:204` — the only `ReportKind` in the workspace. So `bynk check` on an unused `given` capability prints `Error: …` and exits 0. Three lines. Related and nearly as cheap: `run_check`'s `Ok` arm never consults `short`, so a clean-but-warning project build emits a line the VS Code problem matcher cannot parse, and project warnings carry no line or column at all because `ProjectOutput` keeps no snapshots.

Cross-unit diagnostic order is nondeterministic. Fourteen loops iterate `std::collections::HashMap` keyed on unit name and push straight into the `ErrorSink`; `into_all` concatenates without sorting. Two units with errors produce their diagnostics in a different order on different runs of the same compiler on the same source. Changing `groups`, `kinds`, `test_groups`, `integration_groups` and `unit_info` to `BTreeMap` makes every loop deterministic without touching a single loop body. Do *not* sort in `ErrorSink::into_all` instead — the within-unit sequence is load-bearing, per the comment at `validate.rs:895-900`.

`bynk dev` and `bynk deploy` swallow build warnings in the in-process path but print them under the `BYNK_BYNKC` override (`dev.rs:388-405`). `bynk/src/diagnostics.rs` re-exports `print_project_warnings` under `#[allow(unused_imports)]` and nothing in the driver calls it. The inner loop and the last gate before production are the only two commands where `ProjectOutput::warnings`' documented contract is false.

A malformed `bynk.toml` is silently ignored (`paths.rs:95-100`). `read_project_paths` is total by construction and returns `ProjectPaths`, not a `Result`; a trailing comma or `inculde = [...]` produces no diagnostic and the compiler quietly falls back to the conventional layout, after which the user sees a cascade of `bynk.uses.unknown_target` errors pointing at units that plainly exist on disk. This is the one input the user hand-edits that the compiler reads without checking.

Nothing reconciles build output. `write_output` never enumerates or prunes the destination, and the emitted `tsconfig.json` type-checks `**/*.ts` — so deleting a `.bynk` unit leaves its stale emitted `.ts` on disk, still compiled, and `tsc` fails with `Cannot find module './gone.js'` reported as "bynkc test: tsc reported errors against out/tsconfig.json".

Two failure modes deserve mention because they mislead rather than merely fail. In an offline CI container with `npx` present, `bynkc test` spawns `npx --yes -p typescript@5 tsc`, the fetch fails, the process exits non-zero, and the runner reports that *the compiler's own emitted TypeScript is broken*. And cross-file `declared here` labels can underline unrelated text in the wrong file: `Span` has no file identity, `label_fits` can only check in-bounds-and-boundary-aligned against whichever file is being rendered, and the doc comment at `error.rs:161-165` is candid that this "cannot catch a cross-file span that happens to be in-bounds". A `uses`-imported function's name span at bytes 40..43 is in-bounds in the calling file, so ariadne confidently underlines three arbitrary bytes with "function declared here".

---

## Part 3 — Code quality and maintainability

### The two-binary split is half-finished

`bynk-driver` exists (#521) to share command bodies between `bynkc` and `bynk`, and for `check` and `fmt` it does. `test` is the exception, and it is the command where the split costs most. The entire `bynkc test` runner — `run_test` at ~350 lines, plus `finish_runner`, `run_with_coverage`, `run_inspect` and `tool_exists` — lives in `bynkc/src/main.rs`, unreachable from any library. So `bynk test` hand-builds an eight-flag argv and shells `bynkc`, and the argument set is spelled four times: `bynkc::cli::Command::Test`, `bynk::cli::Command::Test`, the standalone `bynk::test::TestArgs` struct, and the argv string literals.

The concrete cost is not the duplication, it is a detection gap. `tool_exists` is `which::which(name).is_ok()` — PATH only. `bynk doctor`'s `detect_runner` goes through `probe::detect`, which prefers `<root>/node_modules/.bin` over PATH and reports npx-provisionable tools as `Warn`, never `Ok`. For the common project with `typescript` as a devDependency and no global `tsc`, doctor reports green and the runner cannot see that binary at all — it falls to `npx -p typescript@5`, type-checking the suite with a different tsc than the project pins. `probe.rs`'s own module doc says it "generalises `bynkc`'s old `tool_exists`", so the portability half was back-ported and the project-local/provenance half was left behind. That is the precise gap, and adopting `probe` inside `bynkc test` is the narrow fix worth recommending. Moving `run_test` into `bynk-driver` is the fuller answer — cargo's `src/bin/cargo/main.rs` is thin dispatch for exactly this reason — but `bynk/src/test.rs:1-11` documents the always-subprocess delegation as a deliberate accepted trade-off, so it should be offered as an option rather than as a correction.

Meanwhile `bynkc`'s library is a pure re-export facade with no consumer. No crate in the workspace depends on it; `bynk/Cargo.toml:24` says so explicitly. It re-exports whole internal modules of eight crates — every `pub` item in `bynk_check::checker` and `bynk_emit::project` — and `bynkc` is published, so `bynkc::checker::Ty` and `bynkc::resolver::*` are public API of a released crate. That re-attaches at the top exactly the constraint the decomposition into leaf crates existed to remove. It also drags `bynk-ide` (8k lines) into every `cargo build -p bynkc` for code the binary never calls. Either shrink the facade to the three modules the binary owns and move `bynk-ide` to dev-dependencies, or curate a real API (`compile`, `compile_project`, `CompileOptions`, `ProjectOutput`, the error types) and `#[doc(hidden)]` the rest. The same over-exposure exists one level down: of thirty-eight world-reachable items in `bynk_emit::emitter`, exactly five have an external user; the rest are `pub` only because `pub mod` promotes sibling-visibility to world-visibility. Eight lines further down the same file already gets this right with `pub(crate) mod source_map; pub(crate) mod emit; pub(crate) mod icu;`.

### `deploy.rs`

At 4,954 lines (2,808 production, 2,146 tests) across 64 top-level functions, `deploy.rs` mixes seven visually banded but structurally undivided concerns: the wrangler.toml model, the committed deploy ledger, the binding graph and topological order, Cloudflare provisioning over `wrangler`, secrets, platform lock, and the plan/apply flow. ADR 0060 sets the author's own trigger — "~2,000 lines is 'eye it'; ~5,000 is 'split it'; and *any* file mixing clearly distinct concerns is a candidate regardless of size" — and names only `emitter.rs` and `parser.rs`, because `deploy.rs` grew in `bynk` after that sweep. It is not the largest file in the workspace (that is `tests_emit.rs` at 6,096), but it has the widest externally visible blast radius: it mints Cloudflare resources and writes secrets. The `project/{paths,discovery,consistency,graph,symbols,…}` split is the template and ADR 0060 is the recipe. Separately, `dev.rs` has become `deploy.rs`'s utility library, which suggests a shared `workers.rs` rather than a dependency between two command modules.

### `declarations.rs` and the six-fold item loop

Three brace/fragment pairs — commons, test, context — total roughly 1,200 of the file's 3,482 lines, and `parse_adapter_body` already demonstrates the unification with a `brace: bool` parameter. The verifier corrected the original claim usefully: the pairs are *not* one-arm diffs (the commons pair differs on 77 lines, the context pair on 119), because the fragment forms also track `last_span` and `seen_item` for `bynk.parse.uses_after_decls` and call `take_epilogue`. But those deltas are exactly the ones worth making uniform: the brace forms' failure to call `take_epilogue` is a live comment-loss bug (`commons x { … }` followed by `-- afterword` loses the afterword to `bynk fmt`), and the divergence is currently invisible as a deliberate choice. The practical cost of six copies is that every new declaration keyword is a six-place edit with no compiler help — `event` had to be added at four of them and is still missing from the recovery sync set.

### Comment trivia, and what the formatter can promise

`split_trivia` builds a complete, lossless comment index keyed by content-token index, and then the parser hands *copies* out through `mem::take` into 34 separate `Trivia` fields on AST nodes. The table itself is private, owned by `Parser`, and dropped when parsing ends — nothing checks whether it was drained. Because expressions carry no trivia, a comment inside an expression is filed against the next content token, never taken, and silently discarded; `bynk-fmt` detects the loss after the fact by re-tokenising its own output and refuses to format the file. That is three parses and four tokenisations per format, and the refusal path has no test in either direction.

Do not adopt a rowan CST for this. The cost is a rewrite of `bynk-fmt`, `bynk-ide` and the checker's AST walks, and the language does not obviously need lossless trees for anything else. Take the cheap 90%: return the `TriviaTable` from the parse entry points alongside the AST, and add `debug_assert!(trivia.is_fully_drained())` at the end of `parse_units_with_warnings`. Because every harvest is already a `mem::take`, the residue *is* exactly the set of lost comments — that one assertion would have failed on both the epilogue drop and every expression-interior comment on the day they were introduced. This is the same registry-plus-assertion pattern the author already runs twice, in `diagnostics.rs` and `keywords.rs`.

### Error recovery, and what the LSP actually sees

Two parser-side defects collapse the editor's partial tree, and one of them fires on *syntactically perfect* files.

`recover_to_top_item` (`parser.rs:502`) is a flat forward scan with no brace-depth tracking that stops at a bare `RBrace`. An error inside a function body syncs forward to that function's own closing brace, the item loop reads it as the end of the enclosing unit, and the unit is returned with zero items — after which the recovery loop re-enters at the next declaration keyword and emits a spurious `bynk.parse.expected_unit_header` plus further garbage. So in the case that actually matters, mid-edit inside a body, document symbols and completion go blank for the rest of the file. The sync token list has also drifted: it omits `Property`, `Actor`, `Event` and `Binding`, all of which the item loops dispatch on. Both fixes are contained: a depth counter, and a shared `fn is_item_start(kind) -> bool` used by both the sync scan and the item-loop dispatch.

Separately, `parse_unit_with_recovery` — `bynk-ide`'s *only* parse entry point — parses every unit after the first and then throws it away (`Ok(_) => {}` at `parser.rs:174`). The loop exists precisely because "a file may hold more than one top-level unit (an atomic `commons` + `suite` file, DECISION S)". The back end does not have this problem; `discovery.rs:226` calls `parse_units_with_warnings` and keeps the vector. So on the atomic file shape the design treats as normal, the entire IDE surface goes dark over the `suite`, on clean input, with no error to hint at why. Adding `parse_units_with_recovery` beside the existing function is the same body with one arm changed, and keeping the old name as a one-line wrapper preserves all sixteen call sites.

### The IDE's two project models

`diagnose_project_with` runs an overlay-aware analysis that parses every project file with the editor's unsaved buffer applied. Alongside it, completion and signature help run a second, independent model: `for_each_unit(doc_text, files: &[PathBuf], …)` iterates *paths* and calls `cached_project_unit`, which does `fs::metadata` plus `fs::read_to_string` and caches by mtime and length. The signature takes `&[PathBuf]`, so there is no way to hand it overlay text at all. Its own doc says "only `doc_text` — the buffer under the cursor — is parsed fresh each call". There are ten production call sites, backing consumable units, capabilities, record fields, sum variants, actors, types and signature help.

The mitigation the code relies on — a first-name-wins dedup that prefers the live buffer — does not hold for the callbacks that dedup on a sub-name. `record_field_names` and `sum_type_variants` key their `seen` set on the field or variant name rather than the unit, so the buffer's `type T` and the disk copy of the same `type T` both contribute and the result is their union: delete a field in an unsaved buffer and completion still offers it. `capabilities_of_unit` accumulates into a `BTreeSet` with no first-wins guard at all. And `for_each_unit` never filters the cursor's own path out of `files`, so the current file is parsed twice per keystroke, once live and once stale.

Two related items. Local references, highlight and hover match any `Ident` token whose text equals the target's name within the target's scope, with no previous-token check — and `.` is a standalone token, so `order.total` lexes as `Ident Dot Ident` and the trailing `total` is byte-identical to a bare read. The author has already reasoned about this collision class once, for agent state fields (`symbols.rs:98-105`), and the parity argument made there is correct for a bare ident and silently false for a post-`Dot` one. The right fix is a `record_use` hook at `Ctx::lookup` (`checker.rs:1400`), the single point where the checker resolves a bare identifier, rather than a token-text heuristic.

And the whole analysis path is a cold compile per keystroke, including re-lexing and re-parsing the seven embedded first-party units on every invocation (`project.rs:1074` and six siblings). Do not adopt a query engine for this yet — salsa's demand-driven model is the eventual right answer but would invert the pipeline, and the phase sequence is not stable enough to pay that. Two bounded increments buy most of it: a `static OnceLock<Vec<ParsedFile>>` for the first-party units, which are pure functions of a `const &str` and speed up builds too; and moving the `canonicalize()` at `discovery.rs:26` behind the overlay lookup so the overlay path does not pay a syscall per file per keystroke. The verifier's caveat on the first: the memo must be per-source, because the injections are individually gated on `consumes`/`uses`, and each run still pays a deep clone unless the `Rc` change below lands.

### Allocation and the quadratic term

`UnitInfo` landed as an *addition*, not a replacement. `assemble_unit_info` clones each producer map, and its own comment explains why: "The producer maps are cloned, not moved, because the back half of the pipeline … still reads the originals." So `RunChecks::Checked` carries both representations, and the per-unit loop then converts *back*: `check_unit_files` opens by rebuilding four whole-project maps from `unit_info`, once per unit. A `UnitTable` is ten `HashMap`s of owned `TypeDecl`/`FnDecl`/`ServiceDecl`/`AgentDecl`, and `FnDecl` owns its `body: Block` — so each of those clones is a deep copy of every declaration body in the program, performed once per unit.

The verifier found that the fix is cheaper than the finding implied: the rebuild in `check_unit_files` is unconditional, but its only consumer (`build_cross_context_info`) is called only for contexts and adapters. Sinking the four `let`s into that branch is a four-line diff that removes most of the term immediately, since every commons unit — including all seven injected first-party units — currently pays the full copy and discards it.

Below that, the per-file emit prologue rebuilds five unit-invariant tables and one project-invariant one on every emitted file, including `collect_history_target_agents`, which is a project-wide fold producing an identical `HashSet` each time. Hoisting those into an `EmitUnitCtx` computed once above the file loop is mechanical.

This matters beyond the cycles because it blocks something already planned. `runtime_use.rs:65-77` notes that "per-unit emission is the obvious thing to parallelise, and that is when it will bite" — but the loop's per-iteration prologue is currently an O(project) deep copy, so rayon would multiply allocator pressure rather than reduce wall time. Sequence it: hoist the rebuilds, then make the remaining per-file copies free with `Rc<TypeDecl>`/`Rc<FnDecl>` inside `UnitTable`, `ResolvedCommons` and `TypedCommons` (this is queue item 12's `Rc` option and does not touch the published AST types), and only then flip `RuntimeUse`'s `Cell` fields to `Atomic` as its comment prescribes. Full symbol interning is not justified yet: the maps are keyed by unit and type names with tens of entries, so hashing is not the cost — the declaration copying is.

One free win on the same axis: `ExprKind::Observation` inlines a ~168-byte struct and sets the size of every expression node. Boxing it and `ExprKind::Is`'s pattern field takes `Expr` from 176 to 120 bytes and `MatchArm` from 488 to 376 — measured, not estimated. A `const _: () = assert!(size_of::<Expr>() <= 128);` pins it so the next large variant is a compile error. (The original finding also proposed boxing `Val`; the verifier measured that it buys nothing.)

---

## Part 4 — Testing, which gates everything above

Almost every recommendation in this review is a refactor, and the review's most important structural finding is about whether refactors are safe here.

The in-file test-line ratio is bimodal and tracks whether a crate has a value-in/value-out API: `bynk-check` 5.8%, `bynk-emit` 6.5%, `bynk-syntax` 8.6%, against `bynk-ide` 36.9%, `bynk-render` 38.6%, `bynk` 30.6%, `xtask` 32.0%. Six files in `bynk-emit` totalling 5,059 lines have literally zero in-file tests — `serialisation.rs`, `workers_entry.rs`, `workers.rs`, `discovery.rs`, `consistency.rs`, `project/diagnostics.rs`. `lower.rs` is 5,068 lines with a 76-line test module; `validate.rs` is 4,494 with 55.

The reason is a missing seam, not missing intent, and the seam is already 90% built. `compile_in_memory` (`project.rs:444`) drives the *entire* project pipeline from a `&str` with zero filesystem access: it builds a one-entry overlay and passes `discovered: Some(…)` into `run_checks`, which already takes `overlay: &HashMap<PathBuf, String>`. A complete virtual filesystem is plumbed through the back end. But `compile_in_memory` hard-codes exactly one file, and `CompileOptions` — the struct the v0.29 track introduced as the single public entry — has no sources field. The options collapse stopped at the public boundary; the twelve-positional-argument `run_checks` underneath did not change.

The author has already found this seam and written down why it matters, once. `emit_bundle` (`emitter.rs:3679`) reaches for `compile_in_memory` with the note: "Without this seam the ICU condition has no crate-local coverage at all — it rests entirely on `bynkc`'s fixtures one crate up, which is a long way from the code that decides the import." That is the only test consumer of it in the workspace.

Adding `sources: Option<HashMap<PathBuf, String>>` to `CompileOptions` and threading it into the existing `overlay`/`discovered` arguments would give `bynk-emit` crate-local, disk-free coverage of roughly 17k lines that today are reachable only from `bynkc`'s on-disk fixtures. Two disk reads bypass the overlay and would need making overlay-aware too (`project.rs:1393` reads an adapter's TS binding module, `paths.rs:96` reads `bynk.toml`). Promoting `emit_source`/`emit_bundle` out of their test module into a `#[cfg(test)] pub(crate) mod testkit` is a ~25-line move that lets every emitter submodule write `assert!(emit_source(src).contains(…))`.

This matters specifically because of what ADR 0059 makes normative: "The acceptance gate is the existing golden fixtures passing byte-identical and unedited." For `project.rs` and `checker.rs` that worked, because their movers were pure string and path helpers. `emitter.rs` and `lower.rs` have almost no pure helpers — their units are `fn(&mut String, &Ast, &mut LowerCtx)` — so the next split will land with whole-file goldens, one crate up, driven from disk, as its only net. When a golden breaks during that split, the diff is an entire emitted TypeScript file and the failing test lives in `bynkc`, not in the crate being refactored.

Two adjacent gaps are worth closing at the same time. The fixture format offers only two assertion granularities and nothing between: whole-file byte identity for emission, and category strings for diagnostics. The author has traced two escaped defects directly to the diagnostic side — `project.rs:5013-5017` ("`expected_error.txt` asserts *category strings only*, never a path … which is precisely why the identity collision survived to slice 0") and `bynk-driver/tests/project_diagnostics.rs:1-6` (#696, same cause) — and both repairs were bespoke Rust tests with their own hand-rolled `Scratch` harness rather than a widening of the format. A third instance sits in production code at `project.rs:244`. An optional `expected_contains.txt`/`expected_absent.txt` (FileCheck applied to rustc's `tests/codegen`) and an optional `expected_diagnostics.txt` carrying `category<TAB>path:line:col` would each be a small change to the existing runner and would have caught both documented escapes at fixture level.

The verifier corrected one part of this worth repeating, because it changes the framing in the author's favour: `workers.rs:481-486` was cited as evidence of being stuck with golden-only pinning, but the full comment describes a state the author *left behind* — the emission was changed specifically so `tsc --strict` becomes the guard instead of the goldens, and `runtime_use.rs:57-64` makes the same move explicitly. That is a pattern of actively escaping golden-only verification, not of being trapped by it. Which makes the one place where the `tsc --strict` backstop is disarmed more notable: emitted test modules destructure every target name through `as any` (`tests_emit.rs:4493`, `:4528`, `:4558`), almost certainly because the destructured name set mixes type-only names with value names. Splitting into a value destructure plus `type X = ns.X;` aliases would restore the gate over the largest emitter file's output — the code developers iterate on fastest.

Finally, two smaller items. The fuzz targets are unowned: three source comments cite them as standing guards ("found by the `parse` fuzz target", "Fuzz-found (#516)", "a fuzz invariant"), the word "fuzz" appears in no design document, and the roadmap's otherwise exhaustive CI inventory does not list them. One weekly cron job with a committed minimal corpus, plus a five-line third target over `compile_in_memory` at `BuildTarget::Workers` — the configuration that reaches `workers_entry.rs`, `wrangler.rs` and `tests_emit.rs` — would make them real again. And `bynk-strip`'s erasability invariant (ADR 0136) is stated as a whole-compiler property and tested only against six hand-written snippets; calling `strip_project_to_js(output)` in `tsc_verify.rs` after each fixture compiles is pure Rust with no measurable cost and converts a comment into a gate. Note that `tsc --strict --noEmit` passing does *not* imply erasability: `enum`, `namespace` and constructor parameter properties all type-check cleanly and are exactly what `strip_types` cannot erase.

---

## What I would do, in order

**First, because they are wrong and cheap.** The lambda `writes_state` arm (`emitter.rs:732`); the store-field shadowing guard in the checker's MethodCall chain and its mirror in the emitter; `local_names_for_handler` at `validate.rs:3109` and `:3197`; `#[derive(Clone)]` on `CompileOptions` and `options.clone().target(Workers)` in `run_test`; the `let` save/restore in `linearity.rs`; `ReportKind::Warning`; the five dead `EmitProjectCtx` fields. Each of these is under an hour and several are one line. Every one of them wants a fixture, because in each case the absence of one is why it survived.

**Second, the drift pins, because they stop the bleeding.** Generate the `method_not_found` message from `LIST_METHODS` so the kernel registry cannot fall behind again. Add the trivia drain assertion. Delete `type_refs_match` in favour of resolved-`Ty` comparison, and make any surviving syntactic comparison exhaustive. Add `serialise_ref_via` and delete `http_value_serialiser` and the duplicated `inner_ts_name`. Convert the three remaining hand-rolled `Expr` walks to `expr_children`. Switch the unit-keyed maps to `BTreeMap`. These are all small, and collectively they close four already-drifted duplications and pre-empt several more.

**Third, the test seam, because everything after it depends on it.** `sources` in `CompileOptions`, the two overlay-aware disk reads, and the `testkit` promotion. Then the two fixture-format extensions. This is the item I would prioritise above any structural refactor, because ADR 0060 names `emitter.rs` as the next split and that split currently has no crate-local net.

**Fourth, the typed hoist.** `Lowered { pre, expr }` through `lower.rs`, which closes the dropped-statements bug, the spliced-statements bug, the match-discriminant miscompile and the short-circuit violation in one change, and makes the `debug_assert!`s unnecessary. Do the `maybe_async_iife` flag at the same time — same file, same idea, and `RuntimeUse` is already the in-house precedent for both.

**Fifth, the layering, in the cheap order.** Move `icu.rs` and `websocket::analyse_open_shape` down to `bynk-check`. Give `ResolvedCommons` a real constructor with private fields. Add `check_body` and demote `Ctx` to `pub(crate)`. Demote the accidental `pub` surface in `bynk_emit::emitter`. Decide what `bynkc`-the-library is for. None of these requires committing to the larger question of whether `validate.rs` should move crates, and all of them improve the answer if you later decide it should.

**Then, at leisure.** `LowerCtx` split by lifetime; the `StoreField` enum; `deploy.rs` and the `declarations.rs` item loops; the `Rc` change and the hoisting that precedes parallelism; `ExprKey` and the uniqueness assertion; the IDE's second project model.

Two things I would *not* do. A rowan CST — the assertion plus returning the trivia table gets the formatter what it needs at a small fraction of the cost, and nothing else in the language obviously wants lossless trees. And a query engine — the incrementality gap is real but the two bounded memoisations above address the measurable part, and inverting the pipeline before the phase sequence has settled would be paying a large fixed cost for a problem that does not yet bite.

## What was proposed and rejected

For completeness, since a review's omissions are informative: the verification pass killed or substantially narrowed several plausible-sounding findings. A full `NodeId` retrofit, a diagnostic-enum rewrite with `thiserror`/`miette` (434 variants in one enum, and `miette`'s renderer would displace the ariadne setup already tuned for byte indexing), a workspace-wide `CodeWriter` conversion (the shelving note's measurement still holds for `emit.rs` and friends — 128 literal-indent sites against 4 computed; it is inverted only in `lower.rs`, which did not exist when the note was written), collapsing the two CLI format enums (`bynkc/src/cli.rs:32-35` gives an on-the-record reason not to), and the claim that `bynkc check` is silent on test-body type errors (`process_tests` runs in every mode; the real gap is attribution-only, and it is already logged as #696).

---

## Appendix — full findings index

Seventy-two findings, grouped by area. "Verdict" is the adversarial verifier's judgement: *confirmed* means the claim survived unchanged, *partly* means it survived with the correction noted in the body above, *verified separately* means the batch verifier failed and the finding was re-checked by a dedicated agent afterwards.

### Emission engine (`bynk-emit/src/emitter*`)

| # | Finding | Sev | Verdict |
|---|---|---|---|
| 1 | `lower_expr`'s pre-statement side channel is untyped: statements dropped or spliced into expressions | High | confirmed |
| 2 | `maybe_async_iife` decides async-ness by scanning generated text; only one of three IIFE builders uses it | High | confirmed |
| 3 | `&&`/`\|\|` operand hoisting defeats short-circuit evaluation, a documented safety property | High | partly |
| 4 | `record_span(out.len(), …)` has no idea which buffer `out` is — IIFE-local offsets corrupt the source map | Medium | confirmed |
| 5 | `LowerCtx` is 48 fields configured by post-construction assignment at 15 sites, no completeness check | Medium | confirmed |
| 6 | Expression-bodied lambda does not rebind `return_ty`, so `?` uses the enclosing function's embedding | Medium | confirmed |
| 7 | The `CodeWriter` proposal was shelved on a measurement that `lower.rs` inverts — re-scope to `lower.rs` | Low | partly |

### Build orchestration (`bynk-emit/src/project*`)

| # | Finding | Sev | Verdict |
|---|---|---|---|
| 8 | `type_refs_match` is a second, syntactic type equality returning false for half of `TypeRef` | High | confirmed |
| 9 | `bynk-check` does not check contexts; the real context checker is 4.5k lines inside `bynk-emit` | High | confirmed |
| 10 | `UnitInfo` landed alongside the nine parallel maps rather than replacing them; table cloning is quadratic | Medium | confirmed |
| 11 | Cross-unit diagnostic order is nondeterministic — `groups`/`kinds`/`unit_info` are std `HashMap`s | Medium | confirmed |
| 12 | `Roots` still models a src/tests role split v0.113 removed; drops `include[2..]`, mis-reports suite locations | Medium | confirmed |
| 13 | A malformed or mistyped `bynk.toml` is silently ignored, with no diagnostic | Medium | confirmed |

### Codegen satellites (`tests_emit.rs`, `workers*`, `serialisation`, `icu`, `secrets`)

| # | Finding | Sev | Verdict |
|---|---|---|---|
| 14 | The test sublanguage's type checker lives in the codegen crate | High | partly |
| 15 | `http_value_serialiser` is the surviving parallel serialise dispatch; drops root `Float`/`Bytes` to a bare cast | High | confirmed |
| 16 | Agent handler bodies checked against an over-wide `local_type_names`, disabling three owner checks | High | confirmed |
| 17 | ~300 lines of the harness's TypeScript runtime are Rust string literals, beside an `include_str!` of `runtime.ts` | Medium | confirmed |
| 18 | Emitted test modules destructure every target name through `as any`, disarming the `tsc --strict` gate | Medium | confirmed |

### CLI and driver (`bynkc`, `bynk-driver`, `bynk`)

| # | Finding | Sev | Verdict |
|---|---|---|---|
| 19 | `bynkc test`'s workers overlay rebuilds `CompileOptions` from scratch, dropping `contracts` and `import_ext` | High | confirmed |
| 20 | The whole `bynkc test` runner lives in `main.rs`; it cannot be shared and detects tools worse than `bynk doctor` | High | confirmed |
| 21 | `bynk dev` and `bynk deploy` swallow build warnings in-process but print them under `BYNK_BYNKC` | Medium | confirmed |
| 22 | `deploy.rs` is 2,800 production lines mixing seven concerns; `dev.rs` has become its utility library | Medium | confirmed |
| 23 | `bynkc`'s library is a pure re-export facade with no consumer, yet published, dragging `bynk-ide` into the binary | Medium | confirmed |
| 24 | `bynk-driver` shares command bodies but not clap surfaces; three commands mirrored by hand via strings | Low | partly |
| 25 | No build-output reconciliation; stale emitted `.ts` survives rebuilds and the tsconfig type-checks `**/*.ts` | Medium | confirmed |

### Front end — syntax (`bynk-syntax`)

| # | Finding | Sev | Verdict |
|---|---|---|---|
| 26 | Comment trivia copied out of a complete index into 34 AST fields; the index is dropped unchecked | High | confirmed |
| 27 | Error recovery is delimiter-blind and its sync-token set has drifted, collapsing the LSP's partial tree | High | confirmed |
| 28 | `Span` is used as node identity; a key miss silently emits `unknown` into the TypeScript | High | confirmed |
| 29 | Six near-identical unit-body loops in `declarations.rs`; the seventh was already unified with `brace: bool` | Medium | confirmed |
| 30 | The LSP's only parse entry point discards every unit after the first — on clean files | Medium | confirmed |
| 31 | Two test-only `ExprKind` variants set the size of every expression node | Low | partly |

### Front end — semantic analysis (`bynk-check`)

| # | Finding | Sev | Verdict |
|---|---|---|---|
| 32 | `checker::Ctx` is public API; `bynk-emit` hand-builds the checker's 24-field working context at three sites | High | confirmed |
| 33 | `expr_types` rides the `Ok` payload while its four sibling analyses are sinks, forcing four rescue calls | High | confirmed |
| 34 | The kernel-method surface exists in five hand-synced copies and `LIST_METHODS` has already drifted | High | confirmed |
| 35 | Store-field method dispatch ignores local shadowing, and the emitter agrees with it | High | confirmed |
| 36 | Store fields are five parallel maps in `Ctx` and five near-duplicate op checkers | Medium | partly |
| 37 | `resolver.rs` threads nine positional parameters through a 900-line walk; 313 of 2,346 lines are argument names | Medium | confirmed |
| 38 | Linearity pass loses a held value's disposal obligation when a binding name is reused in the same block | Medium | confirmed |

### Crate layering and API surface

| # | Finding | Sev | Verdict |
|---|---|---|---|
| 39 | `bynk-lsp`'s "does not depend on `bynk-emit`" seam does not exist; the LSP compiles ~25k lines of TS codegen | High | confirmed |
| 40 | The real `bynk-emit` → driver contract is an untyped file format duplicated across three crates | High | partly |
| 41 | The `bynkc` facade re-exports eight crates' internals and nothing in the workspace depends on it | Medium | partly |
| 42 | ~33 of 38 world-reachable items in `bynk_emit::emitter` are `pub` only to reach a sibling module | Medium | confirmed |
| 43 | `ProjectAnalysis` and `ProjectDiagnostics` are a hand-maintained field-for-field twin (three-site edit) | Medium | confirmed |

### Diagnostics architecture

| # | Finding | Sev | Verdict |
|---|---|---|---|
| 44 | The ADR 0117 warning channel is half-wired: warnings print as "Error", no line/col, ignoring `--format short` | High | confirmed |
| 45 | `category` is a label, not a type: one code carries 42 hand-written templates the registry documents as one sentence | High | confirmed |
| 46 | Cross-file `declared here` labels can underline unrelated text in the wrong file | High | partly |
| 47 | Every renderer except ariadne and the LSP discards `notes` and `labels` — 339 notes, 87 labels invisible | High | confirmed |
| 48 | ADR 0100 kept flattening out of `bynk-render` but left four independent copies with four different fallbacks | Medium | partly |
| 49 | ADR 0054 structured suggestions never reach the CLI; the fix text is hand-duplicated as a note | Medium | confirmed |
| 50 | Severity is a hardcoded six-arm match with no link to the registry; the public reference has no severity column | Medium | confirmed |

### Data representation and performance

| # | Finding | Sev | Verdict |
|---|---|---|---|
| 51 | `UnitInfo` added beside the parallel maps; the back-conversion is O(units²) deep AST clones | High | confirmed |
| 52 | Diagnostic order is `HashMap` iteration order, so multi-unit output is not reproducible run to run | Medium | confirmed |
| 53 | The per-file emit loop recomputes unit- and project-invariant tables on every file | Medium | confirmed |
| 54 | Three hand-rolled `Expr` recursions still shadow `ast::expr_children`, one keeping a `_ => {}` | Medium | confirmed |
| 55 | The LSP path is a full cold compile per keystroke — no memoisation, not even for the embedded first-party units | Medium | partly |
| 56 | Five of `EmitProjectCtx`'s 28 fields are write-only, computed per emitted file and never read | Medium | confirmed |

### Testing and verification

| # | Finding | Sev | Verdict |
|---|---|---|---|
| 57 | `bynk-emit` has no in-process test harness; the fs-free seam that would be one exists and is used once | High | confirmed |
| 58 | Fixture assertions have only two shapes; the author has traced two escaped bugs to the gap | High | partly |
| 59 | The fuzz targets are unowned — absent from every design doc and the CI inventory; `compile.rs` stops before the back end | Medium | partly |
| 60 | Two independent TS→JS lowerings ship in-tree with no differential test; ADR 0136's invariant is checked nowhere | Medium | confirmed |
| 61 | No coverage instrumentation of the compiler's own Rust — the blind spot a byte-identical golden gate cannot see | Medium | partly |

### IDE reuse and incrementality

| # | Finding | Sev | Verdict |
|---|---|---|---|
| 62 | The IDE keeps two disagreeing project models: an overlay-aware analysis and a disk-only parse cache | High | verified separately |
| 63 | Local references/highlight/hover match `.field` tokens as locals — the sink records bindings but not uses | High | verified separately |
| 64 | `bynk check` on a project runs the bailing, emitting `compile_project`, reporting fewer errors than the editor | Medium | verified separately |
| 65 | Every IDE query re-lexes and re-parses from `&str`; the analysis re-parses the first-party surface each round | Medium | verified separately |
| 66 | `bynk-fmt` refuses to format any file with a comment inside an expression, at 3 parses per format to detect it | Medium | verified separately |

### Duplication and drift risk

| # | Finding | Sev | Verdict |
|---|---|---|---|
| 67 | Three hand-rolled partial AST walks sit beside an exhaustive walker in the same file; one drops durable writes | High | verified separately |
| 68 | The `kernel_methods` registry has drifted from the checker: `join`, `joinOn`, `leftJoin`, `groupBy` missing | High | verified separately |
| 69 | Three copy-pasted `file_mentions_*` probes have diverged on three axes, including the ADR 0183 `App` recursion | Medium | verified separately |
| 70 | Refinement predicate → runtime check and message written twice, pinned only by a comment; `Matches` anchoring differs | Medium | verified separately |
| 71 | "Value-keyable (ADR 0110 D5)" is implemented four times across two crates; the fourth miscompiles rather than rejects | Medium | verified separately |
| 72 | The `bynkc` CLI contract exists four times, one copy across a process and version boundary | Medium | verified separately |
