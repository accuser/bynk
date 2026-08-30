# The compiler after the trajectory — a post-restructuring review

**Reviewed at:** v0.289.57, 30 August 2026 (`8bde873`)
**Scope:** the whole compiler pipeline as it stands the day
[`bynk-compiler-trajectory.md`](../bynk-compiler-trajectory.md) declared its endpoint reached —
`bynk-syntax` → `bynk-check` → `bynk-ir`/`bynk-lower` → `bynk-emit`/`bynk-ts` → `bynk-strip`, plus
`bynk-project`, `bynk-ide` and the `xtask` gate suite that certifies the whole thing.
**Reference:** [`bynk-greenfield-compiler.md`](../bynk-greenfield-compiler.md)'s 130 rules, and the
[27 July 2026 pipeline review](2026-07-27-compiler-pipeline-review.md) this one is the sequel to.

---

## How this review was produced

The tree was built, linted and tested first, so nothing below rests on reading alone: `cargo build
--workspace --all-targets` is clean, `cargo clippy --workspace --all-targets -- -D warnings` is
clean, `cargo test --workspace` is **1,992 passed, 0 failed**, and `cargo xtask greenfield-status`
reports *table current* with all fourteen gated probes at their recorded values. That is the
baseline for everything here: **no finding in this review is a broken build, a failing test, or a
tripped gate.** Every one of them is invisible to the suite that is green.

The method was to take the trajectory's own claims one at a time and ask the tree whether they are
true — not "does the type exist" but "who calls it". Where a number is quoted, the command that
produces it is in the appendix, so a later reader can re-measure rather than trust this document (the
discipline `reviews/README.md` asks for).

---

## Headline

**The restructuring worked, and it worked unevenly, and the gates cannot tell the difference.**

Phases 0 through 5 and phase 7 delivered real, load-bearing change. `expr_types` is
`HashMap<ExprId, TypedExpr>` (`bynk-check/src/checker.rs:650`) — the span-collision defect class the
July review opened on is *gone*, not mitigated. `Ty::Error` and `certify` exist and are used
(`checker.rs:832`, `checker.rs:2445`). `bynk-project` is a real crate with a real boundary. The
`bynk-ide` → `bynk-emit` edge is deleted. Diagnostic ownership moved decisively: `bynk-emit`
originated 200 `bynk.*` codes at the 30 July baseline and originates **4** today, with `bynk-check`
at 389 — the semantics live in the crate named for them. And phase 7 is the
one that should be held up as the model — `bynk-emit` builds **1,251** real `bynk_ts` node
constructions across eight files, with exactly **two** `Verbatim` escape hatches left
(`bynk-emit/src/project.rs:2480`, `:2509`). That is adoption, not availability, and it is what the
other phases should be measured against.

Two phases did not do that, and the way they didn't is the same way both times.

**Phase 6 built an IR that the emitter does not consume, and phase 8 built a query layer that
nothing calls at all.** Fifteen of `bynk-lower`'s thirty-two public entry points have zero call sites
outside the crate's own tests. Twenty-one of `bynk-ir`'s forty-one public items have zero consumers
anywhere — and they are not the leftovers, they are the IR: `IrExpr`, `IrExprKind`, `IrStmt`,
`IrPat`, `IrArm`, `MatchForm`, `Exhaustive`, `IrHandler`, `CommitShape`, `IrPredicate`. Phase 8's
entire output — `UnitId`, `UnitSignature`, `ProjectGraph`, `DefId`, `body`, `type_of`, roughly 1,760
lines — is reachable only from its own test files. `bynk-check/src/queries.rs:1-9` says so in its own
first paragraph: *"nothing in the tree calls `body`/`type_of` yet."*

This is P5, the reference's own fifth governing principle, stated verbatim in
`bynk-greenfield-compiler.md:130`: *"When the right shape lands, the wrong one is deleted in the same
change… The recurring failure in the shipped compiler is not a wrong abstraction; it is a correct
abstraction introduced as available while the old path stayed reachable."* The trajectory was
launched to make that failure impossible and reproduced it twice in its last three phases.

The second theme follows from the first. **The gated probes certify that a name exists in a
directory, and a name existing in a directory is not the property any of these rules is about.**
`incremental_query_types` is explicitly *"a one-time existence proof"*
(`xtask/src/greenfield_status.rs:1995`), so phase 8's zero adoption reads green by design.
`ast_importers` is scoped to `bynk-emit/src`, so when `ir.rs`/`ir/lower.rs` were carved into
`bynk-ir`/`bynk-lower` the count fell *without a single import changing* — the probe's own doc admits
it: *"not because they stopped importing the AST (unchanged, still do), but because they left
`bynk-emit/src` altogether"* (`greenfield_status.rs:1441-1443`). Its reading of 5 also rests on two
named exclusions (`greenfield_status.rs:1448`) against a true count of 7, and one of those two files,
`project/tests_emit.rs`, is the same file `ts_writes` had to *un*-exclude after an earlier survey
mischaracterised it as test noise — *"it is `process_tests`/`process_integration_tests`, real
production TypeScript-emission code, not fixture noise"* (`greenfield_status.rs:1678`). The two
exclusions turn on different properties of that file and are each argued, so this is not a
contradiction; it is a probe held at its floor by a hand-maintained list that has already been wrong
about this exact file once.

The third theme is the smallest and the most quietly damning. **The two ungated trend probes — the
ones nobody tuned — both moved backwards over the eight phases.** `keep_in_sync` was the 233 hits
that motivated P2 (`bynk-greenfield-compiler.md:116-119`); it now reads **240**. `wildcard_arms` was the
296-violation inventory T0.3 recorded (`Cargo.toml:54`); it now reads **320**. Both probes use
exactly the definition their baseline did — `keep_in_sync` greps P2's own four phrases, `wildcard_arms`
delegates to clippy — so the comparison is like-for-like in method. It is not like-for-like in
surface: four crates (`bynk-ir`, `bynk-lower`, `bynk-ts`, `bynk-project`) did not exist at the
baseline and their comments and matches now count, which explains part of the rise. What it does not
explain away is the direction. Eight phases aimed at replacing hand-synced facts with types ended
with more hand-synced facts than they started with, and nothing gates either number.

Nothing here argues the trajectory was a mistake. It argues that its last mile was declared complete
on the strength of gates that could not see the thing they were built to protect, and that the
remaining work is now unlabelled — the tracks that would have carried it are retired and their
documents deleted.

---

## Part 1 — Phase 6: an IR nobody lowers to

### 1.1 What is actually adopted

`bynk-emit` names `bynk_lower::` or `bynk_ir::` at 111 sites, which reads like adoption until you
look at which names. Seventeen of `bynk-lower`'s thirty-two public entry points have a production
call site, and every one of them is a small **AST-analysis helper**, not a lowering:
`lower_handler_kind_ir` (23 sites), `lower_handler_given_ir` (13), `lower_protocol_ir_from_commons`
(5), `lower_provider_given_ir` (5), `lower_actor_seam_ir` (2). These take an AST node and answer one
classification question about it. They are useful — they are the deduplication `bynk-ir`'s crate
description promises, *"the small set of shared AST-analysis helpers both `bynk-emit` and
`bynk-lower` need"*. They are not an intermediate representation.

The fifteen with **zero** production call sites are the intermediate representation:

```
lower_expr_ir        lower_block_ir          lower_fn_body_ir       lower_fn_item_ir
lower_handler_ir     lower_service_handler_ir lower_agent_item_ir   lower_service_item_ir
lower_provider_item_ir lower_store_field_ir  lower_commit_shape_ir  lower_invariant_ir
lower_transition_ir  lower_fn_sig_ir_from_types lower_op_sig_ir_from_commons
```

Expression lowering, block lowering, function-body lowering, both handler lowerings, and all four
item assemblies. The emitter's real expression lowering is still
`bynk-emit/src/emitter/lower.rs` — 6,321 lines whose entry point returns `(String,
SourceMapBuilder)` (`lower.rs:31-36`) and which imports `bynk_syntax::ast`'s `Block`, `Expr`,
`ExprKind`, `Pattern`, `Statement` directly (`lower.rs:10-14`). Two lowering passes exist; the one
carrying the tests and the documentation is the one with no callers.

### 1.2 The adopted sites are a detour, not a seam

Where the IR *is* consulted on the hot path, it is consulted and then discarded.
`bynk-emit/src/emitter/workers_entry.rs:380-384`:

```rust
if let bynk_ir::IrHandlerKind::Http { .. } = bynk_lower::lower_handler_kind_ir(&h.kind) {
    let HandlerKind::Http { method, path } = &h.kind else {
        unreachable!("lower_handler_kind_ir is a pure structural mirror")
    };
```

The IR is built, matched on, its payload thrown away, and the same fact re-read from the AST — with
an `unreachable!` standing in for the invariant that the two agree. `workers.rs:606-613` is the same
shape. This is strictly worse than not using the IR: it adds a second dispatch that has to stay in
sync with the first, which is the P2 defect the IR was supposed to retire, introduced by the IR's own
adoption. Both sites carry a candid comment explaining why (the wrappers' signatures are still
AST-typed), which is the codebase at its best — but a documented detour is still a detour.

`bynk-emit/src/emitter.rs:292` and `:482` do the same at item level:

```rust
let IrItem::Type { shape, .. } = lower_type_item_ir(def, program) else {
    unreachable!("lower_type_item_ir always returns IrItem::Type")
};
```

A single-variant round-trip through an enum, unwrapped by `unreachable!`. The function could return
`TypeShape` and the ceremony would vanish.

### 1.3 The IR is not AST-free, and the probe cannot see that either

`bynk-ir/src/lib.rs:163-165` imports `BaseType`, `Block`, `Expr`, `MatchArm`, `Pattern` and
`Refinement` from `bynk_syntax::ast`, and embeds two of them in public IR fields —
`IrPat::Refined { refinement: Refinement }` (`lib.rs:274`), and, in `TypeShape` (`lib.rs:1324-1325`),
`base: BaseType` and `refinement: Option<Refinement>`. `TypeShape` is one of the *adopted* types.
So an emitter reading it touches the AST without spelling `bynk_syntax::ast`, and R6.13's whole
purpose — a firewall between the emitter and the syntax tree — does not hold even where the IR is
used. The `ast_importers` probe says this itself, in its own doc comment
(`greenfield_status.rs:1358-1367`): *"invisible to this probe by construction."*

### 1.4 `lower_expr_ir` would panic if it were wired in

Nine `todo!()`s sit inside the unadopted subtree (`bynk-lower/src/lib.rs:2345, 3273, 3280, 3284,
3288, 3292, 3346, 3521, 3531`). Six are the whole test sublanguage — `expect` in statement and
expression position, `Val[T]`, `Wire`, `Observation`, `trace`. One fires when no `Callee` was
recorded for a call (`:3346`), one when a bare identifier names a free function used as a value
(`:3521`), and one is a defensive catch-all for any other identifier shape (`:3531`). They are inert
today because nothing calls the function.
They are also the honest measure of how far phase 6 got: the IR is not a representation of the
language, it is a representation of the fragment its own single-file test harness could reach.

The cost is not hypothetical. `bynk-lower` is 4,054 production lines against **6,052 test lines** —
66.6% test density, the highest in the workspace by a wide margin, most of it exercising code with no
consumer. `bynk-ir` is the mirror image: 445 code lines under 1,415 comment lines, a 76% comment
ratio and a **0.0%** test density. Between them that is a crate pair carrying the documentation of a
finished design and the tests of a finished design, around a component that is not wired in.

---

## Part 2 — Phase 8: adoption was never in scope

Phase 8 retired on 30 August 2026, closing the trajectory. Its four artefacts:

| Artefact | Lines | Production consumers |
|---|---|---|
| `bynk-check/src/unit_signature.rs` (`UnitId`, `UnitSignature`) | 772 | 0 — `contract.rs:564`'s `canon_unit_signature` is itself only called from `tests/unit_signature_stability.rs` |
| `bynk-check/src/project_graph.rs` (`ProjectGraph`) | 174 | 0 |
| `bynk-check/src/queries.rs` (`DefId`, `body`, `type_of`) | 816 | 0 |
| `bynk-project/src/parse_cache.rs` | 479 | **2 — genuinely adopted** (`discovery.rs:422`, `bynk-ide/src/completion.rs:1525`) |

One of four landed. `parse_cache.rs` is good work — it deletes `PROJECT_UNIT_CACHE` rather than
running beside it (`[DECISION C]`), which is P5 applied correctly, and `[DECISION D]`'s durable
`ExprId` counter closes a real cross-call collision the issue and ADR both missed. That is the shape
the other three should have taken.

The gate cannot distinguish them. `incremental_query_types` checks that the identifiers
`UnitSignature`/`ProjectGraph`/`body`/`type_of` appear as real code somewhere in two crates, that
`PROJECT_UNIT_CACHE` is gone, and that a test with `unit_signature` and `stab` in its name exists. It
reads `4/4`. It would read `4/4` if every one of those types were `pub` and unreferenced — which is
what three of them are.

The trajectory's own closing note is careful about what it does not claim: R3.15's scheduler is
*"deferred whole, not silently dropped"*, tracked as #1523. That is the right posture for the
scheduler. But a query layer with no scheduler is not a deferred decision, it is an unfinished one:
`Body(DefId)` and `TypeOf(DefId)` were commissioned to make editor latency a function of query level,
and `keystroke_latency` still reads *"not measured — no scheduler exists yet"*. The rung below the
missing rung is also missing, and nothing records that.

---

## Part 3 — What the gates certify

Fourteen gated probes, all green, table current. Three of them do not measure their rule.

**`ast_importers` = 5, with two named exclusions and a directory scope.** The real count of
`bynk-emit/src` files importing `bynk_syntax::ast` is 7. The floor of 5 was reached in part by moving
two files into new crates outside the probe's walk (`greenfield_status.rs:1441-1443`, quoted above).
A probe whose reading improves when code is relocated is measuring file layout.

**`ts_writes` = 809, gated at the retirement floor, and the tree has an untagged escape hatch it
cannot see.** `TsExpr::Ident(String)` (`bynk-ts/src/program.rs:812`) carries no doc comment and the
printer emits it raw — `TsExpr::Ident(name) => out.push_str(name)` (`printer.rs:1450`). `bynk-emit`
builds it from `format!` at 15 sites, and what goes in is not identifiers: a quoted string literal
(`emit.rs:400`), a member access (`this.{method}`, `emit.rs:6654`, `:6917`, `:6966`), and a complete
unary expression over an arbitrary predicate — `TsExpr::Ident(format!("!({pred})"))` (`emit.rs:5242`).
`TsLit::Str`, `TsExpr::Member` and `TsUnaryOp::Not` all exist and are bypassed. `TsStmt::Verbatim`
is tagged with a `VerbatimOrigin` and watched by two gated probes; `TsExpr::Ident` is the same hole
with no tag and no probe. This is the one finding here I would fix before anything else, because it
is small, it is local, and every day it stays the tree accumulates more of it under a green gate.

**The `Verbatim` lint has never been run.** `bynk_ts::verbatim_violations` (`bynk-ts/src/lint.rs`)
scans wrapped text for the six constructs R7.1 forbids. `lint.rs:15-18` says *"nothing calls this
over real output yet"* and defers wiring it to Arc C, *"meaningful only once real `Verbatim` content
exists to check."* Real `Verbatim` content now exists — two sites, `project.rs:2480` and `:2509`. The
trigger the comment names has fired; the only reference to `verbatim_violations` in the workspace is
its own `pub use` at `bynk-ts/src/lib.rs:38`.

---

## Part 4 — Defects and residue

### 4.1 `Span::default()` attributes to `FileId(0)`, which is a real file

`FileId` derives `Default` (`bynk-syntax/src/span.rs:15-16`), so `FileId::default()` is `FileId(0)`.
`FileId::UNKNOWN` is `FileId(u32::MAX)` (`span.rs:19`), and the module doc says spans default to it.
They do not: `Span` also derives `Default` (`span.rs:23`), and `Span::default()` therefore yields
`Span { file: FileId(0), .. }`. `FileId(0)` is not a sentinel — `parse_cache.rs:166-167` starts
`next_file_id` at zero, so it is the identity of the first file the process interns.

There are **96** `Span::default()` sites across the workspace, 38 in `bynk-check`, and they include
production diagnostic construction: `bynk.project.no_sources` (`project_model.rs:269`) and
`bynk.project.read_failed` (`project_model.rs:383`). Today this is latent — almost nothing reads
`span.file` (eight sites workspace-wide, none of them a renderer) — but that is the same sentence as
saying R2.2 is not actually in force. The moment a renderer honours `span.file`, which is the entire
point of the rule, these become labels pointing into an unrelated file: defect #46, restored.

The fix is one impl and no call-site changes:

```rust
impl Default for FileId {
    fn default() -> Self { Self::UNKNOWN }
}
```

(dropping `Default` from the derive list on `FileId`; `Span`'s derive then picks it up).

### 4.2 `bynk-emit`'s legacy project analysis is dead, `pub`, and published

`bynk_emit::project::analyse_project` (`project.rs:787`) and `analyse_project_with` (`:797`) have no
production caller: every reach point moved to `bynk_check::analysis::analyse_project` at P4.2
(`bynk-ide/src/lib.rs:322` is the only production call). What keeps them alive is
`bynk-check/tests/differential_analysis.rs`, which compares the new path against them — and to run
it, `bynk-check` carries a **dev-dependency on `bynk-emit`** (`bynk-check/Cargo.toml:25-29`). The
manifest comment is right that this does not invert the library edge. It does mean `cargo test -p
bynk-check` builds the entire emitter, and it means the old path cannot be deleted without deleting
the test that justifies keeping it — a circular anchor. Both functions are `pub` on a published
crate, so they are also API surface.

This is the P5 shape one more time, with a guard rail: the old path stayed reachable, and a
differential test was built to make that safe rather than to make it temporary. The differential test
did its job; the follow-through — delete the legacy entry points, drop the dev-dependency, keep the
fixture as a plain golden test of the new path — was never scheduled.

### 4.3 The comments outlived their citations

This codebase's doc comments *are* its design record, which makes a dead citation more costly here
than elsewhere. **115 source-comment references point at six track documents that no longer exist in
the tree**: `design/tracks/the-ir.md` (57), `semantics-in-the-checker.md` (37), `incrementality.md`
(9), `the-typescript-tree.md` (6), `project-model.md` (4), `content-ownership.md` (2). They were
retired into `design/archive/retired-tracks.md` and deleted; nothing rewrote the citations. A reader
following `design/tracks/the-ir.md §6a` from `workers_entry.rs:375` gets nothing.

Three specific comments are now false rather than merely dangling:

- `bynk-ts/src/lib.rs:26-28` — *"`bynk-emit` still builds no `TsProgram` beyond `Verbatim`"*. It
  builds 1,251 nodes. This is the single most misleading comment in the tree, because it understates
  the best work in it.
- `bynk-strip/Cargo.toml:16` — *"the LSP (via `bynk-ide` → `bynk-emit`) never pulls oxc"*. That edge
  was deleted by P4.2; `ide_emit_edge` reads `absent`.
- `bynk-check/src/checker.rs:684`, `:768` and `bynk-check/src/checker/calls.rs:211` cite
  `bynk-emit::ir::lower`'s `lower_service_handler_ir` / `lower_expr_ir` — a module path that moved to
  `bynk-lower` at P7.12.

Seven markdown links inside `design/` are also dangling (listed in the appendix).

### 4.4 Two things worth recording as correct

`pred_condition_and_message` (`bynk-emit/src/emitter.rs:5260-5340`) was audited for TS injection: the
only user-controlled arm, `PredKind::Matches(pat)`, routes through `escape_ts_string` (`:5335`), and
the sites that build `format!("\"{msg}\"")` without a second escaping pass
(`emit.rs:400`, `serialisation.rs:1012`) document precisely why a `TsLit::Str` there would
double-escape. `e2e.rs`'s `no_unknown_placeholder_in_emitted_output` and the doc-comment escape
fixture `doc_block_tests::escapes_comment_terminator` (`emit.rs:7009-7030`, guarding #720 — a `*/`
inside a doc body closing the JSDoc block early and landing the rest as executable top-level
TypeScript) are the kind of adversarial test most compilers do not have. No injection finding.

`bynk-project/src/parse_cache.rs` is the best-reasoned new file in the restructuring: it deletes what
it replaces, it found and closed a cross-call `ExprId` collision neither the issue nor the ADR
anticipated, and its `[DECISION E]` is an honest account of why one consumer still keeps a local
fallback. One small gap: neither call site canonicalises the path before interning
(`discovery.rs:412` uses `std::path::absolute`, which does not resolve `..`; `completion.rs:1525`
passes the LSP's path through unchanged), so two spellings of one file would intern as two `FileId`s
and two entries. Both callers happen to supply absolute paths today, so this is a latent aliasing
hazard rather than a live bug.

---

## Part 5 — What to do

Ordered by value over cost, not by size.

1. **Tag or close `TsExpr::Ident`.** Split it into `Ident(String)` (identifier, documented as such)
   and a `VerbatimExpr(String, VerbatimOrigin)` that the existing lint and probes can see; convert
   the 15 `format!` sites to real nodes where one exists (`TsLit::Str`, `Member`, `Unary`). Small,
   local, and it stops the leak that phase 7 otherwise closed well.
2. **Run `verbatim_violations` over compiled output.** The trigger its own comment names has fired.
   One call in the e2e harness.
3. **`impl Default for FileId` returning `UNKNOWN`.** One impl, no call-site churn, and it makes the
   96 `Span::default()` sites honest before anything starts reading `span.file`.
4. **Decide phase 6's IR: adopt or delete.** The status quo — two lowering passes, one of which has
   the tests and the documentation and no callers — is the worst of the three options and the one P5
   exists to forbid. If `lower_expr_ir` is to be adopted, the nine `todo!()`s and the AST types in
   `TypeShape`/`IrPat` are the work, and it needs a track. If it is not, delete the fifteen unused
   entry points and the 21 unconsumed `bynk-ir` items and keep the analysis helpers, which earn their
   place. Either way the deferral should be written down the way R3.15's was, with a named trigger.
5. **Same decision for phase 8's three unadopted artefacts**, on the same terms. They are cheaper to
   keep than phase 6's (1,762 lines, no second path) but they are 4/4 on a gate that will keep
   reading green whatever happens to them.
6. **Delete `bynk_emit::project::analyse_project{,_with}`**, convert the differential fixture to a
   golden test of `bynk_check::analysis`, and drop `bynk-check`'s dev-dependency on `bynk-emit`.
7. **Repoint the 115 dead track citations** at `design/archive/retired-tracks.md`, and fix the three
   false comments in §4.3. Mechanical; a `sed` and a review.
8. **Add an adoption probe.** For each `pub` item in `bynk-ir`, `bynk-lower` and
   `bynk-check/src/queries.rs`, does a call site exist outside the owning crate and outside a test?
   That single probe is what this document is a manual run of, and it is the one gate that would have
   made phases 6 and 8 report what they actually shipped. Gate it as a ratchet the way `ast_importers`
   is, so the number can only fall.

None of this is remediation. The compiler is in better shape than it was in July on every axis the
July review named, and phase 7 is a genuinely excellent piece of work. What is left is the last mile
of two phases that were marked finished, and a gate suite that could not tell it was left.

---

## Appendix — how to reproduce every number

Run from the repository root at `8bde873`. Each command prints the figure quoted above.

**Baseline (all green).**

```sh
cargo build --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace                      # 1,992 passed, 0 failed
cargo run -q -p xtask -- greenfield-status  # 14 gated probes; "table current"
```

**Adoption of `bynk-lower`'s public entry points** (17 with a production call site, 15 without):

```sh
grep -n '^pub fn ' bynk-lower/src/lib.rs | sed -E 's/^[0-9]+:pub fn ([a-z_0-9]+).*/\1/' | sort -u |
while read f; do
  n=$(grep -rn --include='*.rs' "$f(" bynk-emit/src bynk-check/src bynkc/src bynk-driver/src \
        bynk-ide/src bynk/src bynk-wasm/src bynk-lsp/src bynk-strip/src 2>/dev/null |
      grep -v '//' | wc -l)
  printf '%-40s %s\n' "$f" "$n"
done
```

**Consumers of each `bynk-ir` public item** (20 used, 21 with none) — same shape, over
`grep -n '^pub \(enum\|struct\|fn\) ' bynk-ir/src/lib.rs`.

**Phase 8's unadopted artefacts:**

```sh
grep -rn --include='*.rs' 'UnitSignature\|ProjectGraph\|UnitId\|queries::body\|queries::type_of' \
  bynk-check/src bynk-emit/src bynkc/src bynk-driver/src bynk-ide/src bynk-lsp/src bynk/src |
  grep -v 'unit_signature.rs\|project_graph.rs\|queries.rs'
```

**TS-tree adoption vs. the `Ident` escape hatch:**

```sh
grep -rc 'TsExpr::\|TsStmt::\|TsDecl::' bynk-emit/src --include='*.rs' -r | grep -v ':0'   # 1,251 total
grep -rn 'TsExpr::Ident(format!' bynk-emit/src --include='*.rs' | wc -l                    # 15
grep -rn 'Verbatim' bynk-emit/src --include='*.rs' | grep -v '///'                         # 2 sites
grep -rn 'verbatim_violations' --include='*.rs' . | grep -v 'bynk-ts/src/lint.rs'          # only the pub use
```

**`Span::default()` sites and the `FileId(0)` default:**

```sh
grep -rn 'Span::default()' --include='*.rs' bynk-*/src | wc -l    # 96
sed -n '14,28p' bynk-syntax/src/span.rs                            # Default derive vs FileId::UNKNOWN
sed -n '160,170p' bynk-project/src/parse_cache.rs                  # next_file_id starts at 0
grep -rn 'span\.file' --include='*.rs' . | grep -v test            # 8 readers, no renderer
```

**Dead track citations in source comments (115 across six deleted documents):**

```sh
grep -rhn 'design/tracks/' --include='*.rs' . | grep -oE 'design/tracks/[a-z0-9-]+\.md' |
  sort | uniq -c | sort -rn
ls design/tracks/     # only events.md and idempotency-capability.md still exist
```

**Dangling markdown links inside `design/`** (7):

```
design/bynk-lsp-spec.md                                  -> ../decisions/0201-the-lsp-analyses-the-compilers-project-model.md
design/decisions/0132-from-websocket-protocol-workers.md -> 0109-agent-storage-staged-commit.md
design/decisions/0133-from-websocket-hibernation.md      -> 0109-agent-storage-staged-commit.md
design/decisions/0199-one-codec-path-at-the-workers-boundary.md -> 0124-agent-state-rehydration.md
design/decisions/0200-cross-context-contract-hash.md     -> 0107-logging-discipline.md
design/archive/bynk-cicd-roadmap.md                      -> bynk-tooling-roadmap.md
design/archive/retired-tracks.md                         -> ../decisions/0203-test-body-service-calls-resolved.md
```

**Crate line counts and comment ratios** (production `src/` only, `#[cfg(test)]` blocks counted
separately for `bynk-lower`):

| Crate | code | comments | ratio |
|---|---|---|---|
| `bynk-ir` | 445 | 1,415 | 76.1% |
| `bynk-ts` | 6,105 | 2,991 | 32.9% |
| `bynk-project` | 1,928 | 811 | 29.6% |
| `bynk-emit` | 28,053 | 11,374 | 28.8% |
| `bynk-lower` | 7,263 | 2,301 | 24.1% |
| `bynk-check` | 32,748 | 8,327 | 20.3% |
| `bynk-syntax` | 12,949 | 3,162 | 19.6% |

`bynk-lower/src/lib.rs` splits 4,054 production lines / 6,052 `#[cfg(test)]` lines.

**Trend probes against their own baselines:**

| Probe | Baseline | 30 Aug 2026 |
|---|---|---|
| `keep_in_sync` | 233 (`bynk-greenfield-compiler.md:116-119`) | **240** |
| `wildcard_arms` | 296 (`Cargo.toml:54`, T0.3 inventory) | **320** |
| `fixture_kinds` | 3 / 2 / 4 vs 419 `expected_error` | 3 / 2 / 5 vs 424 |
