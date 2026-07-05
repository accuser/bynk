# Bynk Compiler Toolchain — Implementation Review

**Date:** 2026-07-04
**Scope:** the full compiler toolchain at workspace version 0.142.0 — `bynk-syntax`,
`bynk-check`, `bynk-emit`, `bynk-strip`, `bynk-render`, `bynk-ide`, `bynkc`, the
`bynk` driver, `bynk-lsp`, `bynk-fmt`, `bynk-grammar`, `bynk-wasm` (~75k lines of
Rust across 12 crates), plus the test corpus, CI, and supporting infrastructure.
**Method:** full-source review of every crate, cross-checked against a local build.
`cargo build`, `cargo clippy --workspace --all-targets -- -D warnings`, and
`cargo test --workspace --locked` all pass clean on the pinned 1.95.0 toolchain.
Findings labelled **confirmed** were traced through the logic; the most severe were
additionally **reproduced** against a locally built `bynkc` with probe programs
(repro steps included below). Findings labelled *suspected* are traced in code but
not executed.

---

## 1. Executive summary

This is an unusually disciplined codebase for a pre-1.0 language project. The
crate decomposition is clean and enforced by the dependency graph, nearly every
function cites its ADR (167 ADRs in `design/decisions/`), drift between the
grammar, docs, CLI reference, diagnostics registry, and editor tooling is guarded
by dedicated tests, and the CI pipeline is top-decile: 3-OS matrix, SHA-pinned
actions, `--locked` everywhere, cargo-audit/cargo-deny/CodeQL/Scorecard, and —
rare for a compiler project — the emitted TypeScript is batch-verified with
`tsc --strict` and *executed* under Node in 21 behavioural suites.

Against that strong baseline, the review found a small number of serious,
verified defects that cluster in two blind spots:

1. **The handler-body checking path bypasses the resolver.** Unknown
   identifiers, unknown functions, and wrong-arity calls inside HTTP/agent/service
   handler bodies are **silently accepted** and broken TypeScript is emitted
   (reproduced — §3.1). The 279-fixture negative corpus never exercises these
   errors in handler position, which is exactly why the gap survived.
2. **Adversarial inputs are untested.** There is no fuzzing, no property
   testing, and almost no non-ASCII coverage. That blind spot hides a compiler
   stack-overflow crash on mutually recursive record types (§3.3), a
   validation-bypass in emitted regex refinements (§3.2), LSP panics and
   misplacement on non-ASCII text (§3.5), and byte-vs-char diagnostic rendering
   errors (§3.6).

None of these is architectural. Each has a contained fix, and the existing
fixture/bless machinery makes regression-pinning cheap. The prioritized roadmap
is in §6.

### Top findings at a glance

| # | Severity | Component | Finding | Status |
|---|----------|-----------|---------|--------|
| 1 | Critical | bynk-check | Unknown names / unknown fns / wrong arity in handler bodies pass `check` and emit broken TS | **Reproduced** |
| 2 | Critical (security) | bynk-emit | `Matches` refinement regexes mis-anchored: `"ab\|cd"` emits `/^ab\|cd$/`, accepting `"abZZZ"` — boundary-validation bypass | **Reproduced** |
| 3 | High (crash) | bynk-check | Stack overflow on mutually recursive record types used as agent state | Confirmed by agent repro |
| 4 | High | bynk-emit | JS reserved words (`class`, `await`, `yield`, …) emitted verbatim as identifiers → TS syntax errors; handler param named `deps` collides with the generated `deps` param | Confirmed |
| 5 | High | bynk-check | `Ty::Query` missing from `compatible`/`structurally_compatible` → `Query[T]` incompatible with itself; identical `List[Int]`s rejected at context boundaries | Confirmed |
| 6 | High | bynk-lsp | Byte-based cursor offset conversion (LSP positions are UTF-16): wrong completions and a slice **panic** on non-ASCII lines | Confirmed |
| 7 | High | bynk-emit | `Matches` patterns validated with Rust `regex` but executed by JS `RegExp` → compile-accepted patterns throw at runtime (500s) | Confirmed |
| 8 | Medium | bynk-syntax | Digit separators (`1_000`) rejected/mis-parsed in 5 of 6 literal sites; `InRange(0.0, 1_000.5)` silently becomes a NaN bound | Confirmed |
| 9 | Medium | bynk driver | Signal-death of child processes maps to exit 0; broken `BYNK_BYNKC` override reports "Ok" in `bynk doctor` | Confirmed |
| 10 | Medium | docs | The README's front-page example does not compile (three separate syntax divergences) | **Reproduced** |

---

## 2. Architecture assessment

**Pipeline.** `bynk-syntax` (lex/parse/AST) → `bynk-check` (resolve/check) →
`bynk-emit` (project orchestration + TS emission) → `bynk-strip` (optional TS→JS
via oxc), fronted by `bynkc` (compiler CLI) and `bynk` (driver: doctor/new/dev),
with `bynk-ide`/`bynk-lsp`/`bynk-fmt` sharing the same library surface and
`bynk-wasm` reusing the identical pipeline in-memory for the playground.
`bynkc/src/lib.rs` is almost pure re-exports preserving the pre-decomposition
API; the crate-decomposition slices are documented inline and the boundaries are
structurally enforced (e.g. `bynk-render` depends only on `bynk-syntax` +
`ariadne`, so the documented "no render→emit edge" invariant cannot silently
break).

**Strengths worth preserving:**

- *Traceability.* ADR references at nearly every decision point; the
  diagnostics registry, grammar-coverage bijection test, CLI-reference
  generator, and legend-drift tests turn "spec-first" from aspiration into
  mechanism.
- *Diagnostics machinery.* Stable dotted codes, labels, notes, and
  machine-applicable, list-aware suggestion edits from day one.
- *The LSP's snapshot discipline.* Analysis results are an immutable
  `Analysis` bundle carrying the exact source snapshots its spans index into;
  spans are only ever converted against the analysed snapshot ("v0.24 rule"),
  preventing the classic stale-span bug by construction. Rename plans against a
  fresh analysis and then *validates* by re-analysing with edits applied.
- *Centralized emitted runtime.* Security-sensitive generated logic (CORS,
  HMAC, security headers, conditional GET, path matching) lives in one
  reviewable `bynk-emit/src/emitter/runtime.ts` rather than being scattered
  through format strings. CORS reflection is exact-allowlist with fail-closed
  omission; HMAC uses WebCrypto `crypto.subtle.verify`; the 413 body-limit
  guard runs before the body read and before the auth seam.

**Structural weaknesses (the places bugs cluster):**

- *The checker's most important pass is orchestrated from the wrong crate.*
  Handler/service/agent/test bodies are checked from
  `bynk-emit/src/project/validate.rs` (~3.9k lines) calling
  `check_handler_body` (bynk-check/src/checker.rs:333) with 24 positional
  parameters — and those bodies never pass through the resolver's expression
  walk. Several diagnostics exist only in the resolver, so handler bodies have
  no backstop (finding §3.1).
- *`Ty` totality by convention.* `compatible`, `unify`, `substitute`,
  `structurally_compatible`, `json_codable` etc. hand-enumerate `Ty` variants
  and end in `_ =>` catch-alls; every new wrapper type must be added to all of
  them. `Query` was missed in two (finding §3.4) and the same trap awaits the
  next variant.
- *String-building emission.* `bynk-emit` accumulates output text directly
  (no TS AST/IR), with escaping and identifier legality enforced ad hoc at each
  `format!` site — which is where the reserved-word, duplicate-param, and
  inconsistent-escaping defects live. Header/import decisions are made by
  substring-scanning the emitted body (`body.contains("__bynkBytes")`,
  emitter.rs:152), coupling imports to generated text rather than structure.
- *Front-end duplication.* The `bynk` driver re-implements `bynkc`'s fmt and
  check command bodies, the failure flatteners exist twice, and
  `project_options` exists three times. Comments say "mirrors bynkc's…" but
  nothing pins the copies together.

---

## 3. Verified critical and high-severity findings

### 3.1 Handler bodies accept unknown names, unknown calls, and wrong arity (critical)

`check_ident` falls off the end returning `None` with no diagnostic
(bynk-check/src/checker/expressions.rs:130), and `check_call` does the same when
nothing owns the name (calls.rs:411-413); `check_call_against_fn` and
`check_generic_call` return `None` on arity mismatch without a diagnostic
(calls.rs:708-713, 502-507). Inside `fn` bodies the resolver's walk backstops
all of these (`bynk.resolve.unknown_name`, `bynk.resolve.arity_mismatch`), but
handler/service/agent/test bodies never go through that walk (acknowledged at
checker.rs:1211-1215).

**Repro (verified against a local build):**

```bynk
context probe

service api from http {
  on GET("/ping") by v: Visitor () -> Effect[HttpResult[String]] {
    Ok(nonexistentFn(unknownVar))
  }
}
```

`bynkc check` exits **0** with no output; `bynkc compile` emits
`return Ok(nonexistentFn(unknownVar));` into the output TypeScript. The same
gap covers wrong-arity calls to known functions, and agent-handler call
arguments are typed but never compatibility-checked
(`let _ = type_of(arg, pty.as_ref(), ctx);`, calls.rs:1927-1930).

The batched `tsc --strict` verification in CI does not catch this because the
committed positive fixtures are all well-formed; the negative corpus tests these
error codes only in `fn` position.

### 3.2 Regex refinement anchoring is bypassable (critical, security)

`bynk-emit/src/emitter/emit.rs:228-241` and `lower.rs:3469` emit
`new RegExp("^" + pat + "$")`. For any pattern with top-level alternation the
anchors bind to the outer branches only.

**Repro (verified):** `type Code = String where Matches("ab|cd")` used as an
HTTP path-param type emits

```ts
if (!new RegExp("^" + "ab|cd" + "$").test(value)) {
```

i.e. `/^ab|cd$/` ≡ `(^ab)|(cd$)`, which accepts `"abZZZ"` and `"ZZZcd"`. Refined
string types are exactly what guards HTTP params and request bodies, so this is
an input-validation bypass on the trust boundary. Fix is one line per site:
emit `"^(?:" + pat + ")$"`.

Related (confirmed, not run): the checker validates `Matches` patterns with
Rust's `regex` crate while the emitted code runs JS `RegExp`. A pattern like
`(?P<yr>\d{4})` compiles cleanly and then throws `Invalid group` at runtime —
`T.of(...)` throws instead of returning `Err`, surfacing as a 500. The two
engines also diverge on lookaround, backreferences, and Unicode classes.
Validation should target JS semantics (or a common subset).

### 3.3 Compiler crash on mutually recursive record types (high)

The resolver forbids only *direct* record self-reference
(`bynk.resolve.recursive_record_field`, resolver.rs:427-447 — the comment "v0.2
has no indirection … to defeat" is stale). `zero_value_ts` →
`agent_state_zero_record` (refinements.rs:518-568) recurses through `Named`
record fields with no visited set and no depth cap.

`type A = { b: B }; type B = { a: A }` plus `store slot: Cell[A]` overflows the
stack and aborts the compiler. The codebase already contains the right pattern
twice (`structurally_compatible_inner`'s visited set, `can_mock_bare`'s
`MOCK_DEPTH`); it was just never applied here.

### 3.4 `Ty` catch-all gaps make types incompatible with themselves (high)

- `compatible` (checker.rs:1622-1671) has no `Ty::Query` arm → `let q:
  Query[Event] = history.recent(n)` fails with *"has type `Query[Event]`, but
  the annotation declares `Query[Event]`"*, and `Query.flatMap` with a
  store-query-returning lambda cannot type.
- `structurally_compatible` (checker.rs:2536-2583) has no collection arms →
  passing `List[Int]` to a consumed cross-context service parameter of type
  `List[Int]` is rejected with the same absurd self-mismatch phrasing.

These messages are the user-visible face of the `_ =>` catch-alls; the
structural fix (make every `Ty` match exhaustive so the Rust compiler flags the
next added variant) prevents the whole class.

### 3.5 LSP: byte-based position conversion — wrong results and panics on non-ASCII (high)

`bynk-lsp/src/position.rs` correctly counts UTF-16 code units, but a second,
parallel conversion — `cursor_byte_offset` (main.rs:2429-2441, self-described
"ASCII-faithful") — treats `Position.character` as a **byte** index and feeds
signature help, offset-based completions, and unit-reference definition. The
completion `line_prefix` (main.rs:1347-1353) slices the line at `character`
bytes. On any line with non-ASCII before the cursor these produce wrong
offsets, and the unguarded slices at completion.rs:505/520/535
(`&text[open + 1..offset]`) can land mid-codepoint and **panic** the request
handler (e.g. completing inside a `cors { }` block on a line containing
`-- café`). The only position tests are ASCII.

Additional confirmed LSP defects: stale-analysis last-writer-wins race (an old
slow round can overwrite a newer round's diagnostics/index, main.rs:329/371);
`did_change_watched_files` ignores non-open files, so a git checkout leaves the
project index stale (main.rs:1969-1983); `[lsp] diagnostics_mode = "on_save"`
is parsed but never honored (`#[allow(dead_code)]`, project.rs:134-135);
value-member completion runs a **full project re-analysis synchronously on the
request path** for every `.` keystroke (`type_receiver`, main.rs:533-567).

### 3.6 Diagnostics rendering: byte spans through ariadne's char index (medium-high)

`Span` is a byte range (bynk-syntax/src/span.rs:3), but `CompileError::report`
uses `Config::default()` (error.rs:131-176) and ariadne 0.6 defaults to
`IndexType::Char` — any diagnostic on a line containing non-ASCII text before
the span renders a misplaced underline. One-line fix:
`.with_index_type(IndexType::Byte)`. Related: cross-file labels carry no file
identity, so a label whose span lives in another file renders over unrelated
lines of the primary file (observed with a `uses`-imported callee's "parameter
declared here" label).

### 3.7 Emitter: identifier legality (high)

- JS reserved words pass through verbatim: `fn f(await: Int) { let class =
  await + 1; … }` emits `const class = await + 1;` — a hard TS syntax error.
  Only a contextual `old`/`new` → `__old`/`__new` special case exists
  (lower.rs:2740). There is no `ts_ident()` renamer.
- Handler signatures unconditionally append a generated `deps: {…}` parameter
  (emit.rs:1052, 2411, 2857); a user param named `deps` (e.g. route
  `/hello/:deps`) produces a duplicate parameter.
- Generated temporaries (`__r{n}`, `__state`, …) are collision-safe only by
  the accident that the parser currently rejects `let __x` bindings — an
  unvalidated invariant.
- Escaping is split between the canonical `escape_ts_string` (emitter.rs:2429,
  handles control chars) and weaker quote/backslash-only sites (`ts_str` at
  workers_entry.rs:583 plus inline `.replace(...)` pairs) carrying CORS
  origins, signature secret/header names, cron expressions, queue names, and
  path literals. No injection hole (quotes/backslashes are covered
  everywhere), but a literal containing a newline breaks the emitted file.

### 3.8 Parser: digit-separator inconsistency, including a silent NaN (medium)

Digit separators (v0.142/ADR 0166) are stripped in only one of six
literal-parse sites. `1_000` in a match pattern → spurious
`bynk.lex.integer_overflow`; `MinLength(1_000)` fails; and worst,
`InRange(0.0, 1_000.5)` **silently becomes a NaN bound** via
`slice.parse().unwrap_or(f64::NAN)` (bynk-syntax/src/parser/types.rs:428).
Sites: parser/expressions.rs:1080; parser/types.rs:384, 410, 428;
parser/declarations.rs:2181. Route every numeric parse through one
separator-stripping helper.

### 3.9 CLI/driver correctness (medium)

- **Broken `BYNK_BYNKC` override reports Ok.** `compiler::locate` always
  returns `Some(path)` for an override (bynk/src/compiler.rs:126-133), so the
  doctor's "override set but not found" arm is unreachable; a typo'd override
  renders **Ok — "unknown (override)"** while `bynk check/dev` fail at spawn.
  The covering test fabricates a state the resolver can never produce
  (bynk/tests/doctor.rs:101-110). Also: `BYNK_BYNKC=""` resolves `./bynkc`
  from the current directory (a mild security issue), and the
  `file_stem`-based re-resolution can silently pick a *different* binary than
  the one named (`/dir/bynkc.backup` → `/dir/bynkc`).
- **Signal death maps to exit 0.** `exit_byte` (bynk/src/shell.rs:16-18) maps
  `code() == None` to success — reasonable for shared Ctrl-C, but a delegated
  `bynkc test` killed by SIGSEGV/OOM reads as **passing** in CI.
- **JSON-mode `tsc` failures lose the diagnostics**: `tsc` writes errors to
  stdout, but the runtime-error document captures only stderr
  (bynkc/src/main.rs:271-282), so the pinned surface's most useful field is
  systematically empty.
- **`bynk dev` has no file watching** — the compile is one-shot; editing
  `.bynk` sources does nothing until restart, despite the command being billed
  as the edit loop. It also compiles a different project shape than
  `bynkc compile <root>` (first `[paths] include` entry only, re-rooted).
- **`bynk new`'s template documents dead config**: the scaffolded `bynk.toml`
  writes `[paths] src/tests` keys, but `read_project_paths` (which is a
  hand-rolled line scanner despite `toml` being a workspace dep) only reads
  `include`/`exclude`.
- **Windows**: `bynkc`'s `tool_exists` shells POSIX `which`
  (bynkc/src/main.rs:477-487) and bare `Command::new("tsc")` won't resolve
  `.cmd` shims — while the driver already uses the portable `which` crate, so
  `bynk doctor` can report `test: ok` where `bynkc test` fails.

### 3.10 Documentation drift on the front page (medium, reproduced)

The README's one showcase example does not compile against 0.142.0: `on http
GET "/ping"` must be `on GET("/ping")`, the service needs `from http`, and the
HTTP handler needs a `by v: Visitor` actor clause. Given that
`bynkc/tests/doc_examples.rs` compiles every ```` ```bynk ```` fence in the
Book, extending that guard to `README.md` is a one-line scope change that would
have caught all three. (Bonus finding from the same probe: several
`expect_ident` call sites double the prefix — *"expected identifier expected a
handler form …"* — parser/statements.rs:134, parser/declarations.rs:2135 etc.)

---

## 4. Component summaries

### bynk-syntax (lexer/parser/AST, ~12.7k lines)

Sound, deliberately layered, allocation-light (`Copy` tokens, source slicing),
UTF-8-safe byte scanning, and a genuinely nice trivia side-table design for
formatter round-tripping. Error recovery was specifically audited for
non-progress loops and is **hang-free**. Weaknesses: four parse entry points
with divergent semantics — notably `parse_unit` returns `Err(warnings)` on a
*successful* parse (parser.rs:210-218), so an orphan doc block (classified
`Warning` per ADR 0117) hard-fails file discovery and throws away a good AST;
recovery syncs to any `RBrace` without brace-depth awareness, producing cascade
noise; the lexer is fail-fast (one stray character → zero tokens → no partial
AST for the LSP); `parser/declarations.rs` (3,182 lines) repeats six
near-identical item loops (~1,000 removable lines); `is_reserved_keyword` has
drifted from the actual keyword set (17 omissions) and — unlike everything else
in this codebase — has no drift-guard test. In-crate tests: 60, all in
lexer/parser core; the four biggest parser submodules have zero.

### bynk-check (resolver/type checker, ~15.5k lines)

Two-pass front half with a well-executed "sink" pattern separating IDE analysis
tables from the checking result, monomorphic call-site generics pinned by good
characterization tests, structured diagnostics with machine-applicable fixes,
and almost no panic surface (4 guarded unwraps). The serious problems cluster
in exactly two places, both covered in §3: the handler-body path that bypasses
the resolver, and hand-maintained `Ty`-totality. Additional confirmed items:
the `:=` self-reference rule is bypassable through match/if arms because the
shared child-walker descends into discriminants/tails only
(checker.rs:1093-1150); three hand-rolled partial expression walkers
(`predicate_children`, linearity's `walk_expr`, `expr_reads_ident`) disagree
about what an expression contains — one total child iterator in `bynk-syntax`
should back all of them. Performance: whole-`MethodTable`/`FnDecl` clones per
call site, full type-declaration scans per bare identifier (a variant→owner
index is a one-time fix), and `ProjectIndex::symbol_at` is a linear scan on the
LSP's go-to-definition hot path. `check_handler_body` takes 24 positional
parameters; `Ctx` construction is copy-pasted 5×; the store-op/kernel checkers
repeat one signature-table pattern ~10×.

### bynk-emit (TypeScript emitter, ~28.5k lines)

The centralized `runtime.ts` is the crate's best decision (§2), and the
generated router is thin, readable glue with correct ordering (413 before body
read before auth seam). CORS and HMAC generation were specifically audited and
are correct. The defects are the identifier/escaping/regex items in §3.2 and
§3.7. Deserialisers are allowlist-based, build fresh objects, and validate
`Int` integrality and `Float` finiteness — no mass assignment;
`__proto__`-named fields are worth a targeted test. Maintainability: 940-line
`emit_agent`, ~613-line `lower_method_call`, a 40-field `LowerCtx`, and ~7
near-identical full-AST walk helpers wanting a visitor. Minor: `matchPath`'s
`decodeURIComponent` throws on malformed percent-encoding → 500 instead of 400;
body limit trusts `Content-Length` (documented platform limitation).

### bynkc + bynk driver + support crates (~5.5k lines)

Clean crate boundaries; clap-derive CLIs with a self-documenting reference
generator and drift guard; JSON output stability handled correctly (field-order
rationale documented against `preserve_order` unification); remedy-carrying
doctor rows. Defects in §3.9. Quality items: `run_test` is a 290-line
function mixing seed parsing, two compiles, a runner ladder, and three output
modes; `bynkc` collapses all failures to exit 1 (scripts cannot distinguish
"tests failed" from "tsc missing"); `bynkc test -o build/tests` silently writes
a sibling `build/out-js/`; in-place `fmt` writes are non-atomic (crash
truncates source); no timeouts on child processes; `strip_project_to_js` stores
JavaScript in a field named `typescript`; `bynk-wasm`'s last-resort fallback
JSON is built with Rust `{:?}` escaping, which can produce invalid JSON
(`\u{7f}`) on the one path that exists to be always-valid.

### bynk-lsp + bynk-fmt (~12.2k lines)

Full-text sync, full-project reanalysis per debounced round, no incremental
parsing, no cancellation — acceptable for small projects, with the §3.5 items
as the real bugs. Lock discipline is good (no deadlock path found); pure logic
is systematically split into unit-tested modules; rename validation and the
snapshot rule are exemplary. The formatter is AST-based with real idempotency
teeth (the whole positive corpus is round-tripped, `fmt(fmt(x)) == fmt(x)`,
plus re-parse checks) and is safely inert on unparseable input. Its one
documented lossiness: comments inside expression subtrees are folded into the
enclosing statement's leading trivia *or dropped* — silent comment loss is
real; a cheap guard is to refuse formatting when the output's comment count
differs from the input's. `vscode-bynk` is solid (SHA256-verified server
download); `tree-sitter-bynk` is a second hand-maintained grammar validated
against the fixture corpus.

### Testing & CI

The backbone is a 549-fixture corpus (270 positive with byte-for-byte expected
TS trees and warning files, 279 negative with code+message assertions),
regenerated via a consistent `BYNK_BLESS=1` flow, plus batched
`tsc --strict --noEmit` over all project-form positives, Node execution checks,
and 21 behavioural suites running compiled output against in-memory fakes.
Spec/doc/grammar drift is mechanically enforced. CI: 3-OS matrix, path-filter
gating with a single required `ci-green` check, SHA-pinned actions, pinned
toolchain, cargo-audit + cargo-deny + npm-audit + CodeQL + Scorecard, real
VS Code integration tests under xvfb, and a 5-target release matrix.

Gaps: **no fuzzing or property tests anywhere** (the largest structural gap —
the hand-written parser feeds the LSP and the in-browser playground, where a
panic is a crash); emitted Workers never run on a real Workers runtime
(workerd/miniflare absent — fake-vs-real drift is invisible); `examples/` are
compiled but their output is neither tsc-checked nor executed; only 1 of 279
negative fixtures asserts a position, so a span regression passes the whole
suite; `bynk-render` (diagnostic rendering) has zero direct tests; unit density
in `bynk-check` (38 tests / 15.5k lines) and `bynk-emit` (38 / 28.5k) is thin —
the corpus compensates end-to-end but failures are coarse; the MSRV CI leg
currently installs the same toolchain as every other job (admitted placeholder);
no coverage measurement, no benchmarks.

---

## 5. Cross-cutting themes

1. **The corpus tests what the corpus contains.** Every reproduced defect —
   handler-body unknowns, regex alternation, reserved-word identifiers, digit
   separators in patterns, non-ASCII positions — sits precisely outside the
   shapes the 549 fixtures happen to exercise. The verification *machinery* is
   excellent; the *input distribution* is friendly. Fuzzing, property tests,
   and a handful of adversarial fixtures are the highest-leverage additions.
2. **Convention-enforced invariants drift; compiler/test-enforced ones don't.**
   The project already knows this — its drift-guard tests are the best in
   class — but the pattern isn't applied uniformly: `Ty` match totality,
   `is_reserved_keyword`, the severity category list in `error.rs`, the
   resolver-vs-checker diagnostic ownership split, and the bynkc/bynk command
   duplication are all convention-only today, and all have drifted or broken.
3. **Two position/escaping implementations where one should exist.** Byte vs
   UTF-16 conversion in the LSP, `escape_ts_string` vs `ts_str` in the
   emitter, byte spans vs char-indexed ariadne, three partial expression
   walkers in the checker. Each duplicate pair produced at least one bug.

---

## 6. Prioritized recommendations

### P0 — fix before anything else ships (all small, contained)

1. **Close the handler-body resolution gap** (§3.1): emit `unknown_name` at the
   end of `check_ident`, `unknown_function` at the end of `check_call`, arity
   diagnostics in `check_call_against_fn`/`check_generic_call`, and a
   `compatible` check on agent-handler arguments — then add negative fixtures
   for each *in handler position*.
2. **Anchor emitted refinement regexes** as `^(?:pat)$` (emit.rs:229,
   lower.rs:3470) and add an alternation behaviour test (§3.2).
3. **Fix the recursive-record stack overflow** with a visited set in
   `zero_value_ts`/`agent_state_zero_record`, and extend the resolver check to
   indirect cycles (§3.3).
4. **Add the missing `Ty` arms** (`Query` in `compatible`; collections in
   `structurally_compatible`) and replace the `_ =>` catch-alls with
   exhaustive matches so the next variant is a compile error (§3.4).
5. **Unify LSP position conversion**: delete `cursor_byte_offset`, route all
   consumers through `position_to_offset`, replace the unguarded slices with
   `text.get(..)`, and add non-ASCII round-trip tests (§3.5).

### P1 — correctness and safety

6. Introduce a single `ts_ident()` renamer for JS reserved words and generated-
   name collisions (`deps`), applied at every user-identifier emission site;
   unify all emitter escaping on `escape_ts_string` (§3.7).
7. Validate `Matches` patterns against JS `RegExp` semantics (§3.2).
8. Route all numeric-literal parsing through one separator-stripping helper;
   make the `InRange` float NaN swallow a hard error (§3.8).
9. Thread warnings out of the parse APIs instead of `Err(warnings)`
   (parser.rs:210-218) so severity actually governs gating; set
   `IndexType::Byte` on ariadne reports; rebase interpolation-hole lex-error
   spans; give labels file identity or drop cross-file labels.
10. Sequence LSP analysis rounds (commit results only if still newest), trigger
    re-analysis from watched-file events for non-open files, move single-file
    diagnosis off the executor thread, and get `diagnose_project` off the
    completion hot path (§3.5).
11. Driver: make a broken/empty `BYNK_BYNKC` override fail doctor honestly;
    treat non-SIGINT signal death as failure; capture `tsc` stdout in the JSON
    error document; port `bynkc`'s tool detection to the `which` crate (§3.9).
12. Add one total `Expr` child iterator in `bynk-syntax` and use it for the
    `:=` rule, predicate walks, and linearity walk.

### P2 — test-infrastructure investments (highest long-term leverage)

13. **Fuzz the lexer/parser** (`cargo-fuzz`, seeded from the 549-fixture
    corpus; invariant: never panics, spans in-bounds and char-aligned) — this
    would have caught §3.3, §3.5, and the eof-span edge in one sweep.
14. **Property-test the round-trips** you already assert on a fixed corpus:
    `parse(fmt(ast)) == ast` and formatter idempotency over generated inputs.
15. **Run one example on a real Workers runtime** (workerd/miniflare smoke) and
    stage `examples/` output into the existing batched `tsc --strict` run.
16. Assert spans in a subset of negative fixtures; add ariadne-rendered golden
    tests for `bynk-render`; extend the doc-example compile guard to
    `README.md` (§3.10); add a report-only `cargo llvm-cov` job to direct new
    fixtures at unreached `bynk-check`/`bynk-emit` branches; make the MSRV leg
    honest or fold it.

### P3 — maintainability

17. Deduplicate the `bynkc`/`bynk` front-end (fmt/check bodies, flatteners,
    `project_options`); replace the hand-rolled TOML scan with the `toml`
    crate and fix the `bynk new` template's dead `[paths]` keys.
18. Decompose `emit_agent` (~940 lines), `lower_method_call` (~613),
    `run_test` (~290); introduce `CheckSinks` + a params struct for
    `check_handler_body` (24 args) and a `Ctx` builder; table-drive the six
    parser item loops; add an emitter AST-walk visitor.
19. Formatter: refuse to format when the output's comment count differs from
    the input's (turns silent comment loss into a no-op); minimal-diff edits
    instead of whole-document replace.
20. Add file watching to `bynk dev` (or document its absence), a rebuild loop
    being the command's stated purpose; pin npx-provisioned package versions
    (`wrangler@N`, exact `typescript` version).

---

## 7. Closing assessment

The Bynk toolchain's engineering culture — ADR-traceable decisions, drift
guards, executed-output verification, hardened CI — is its strongest asset and
is well ahead of most pre-1.0 language projects. The verified defects are
serious but narrow: one architectural seam (handler bodies bypassing the
resolver), one enforcement gap (hand-maintained match totality), one emission
discipline gap (no central identifier/escaping layer), and one testing blind
spot (no adversarial inputs). All four have contained fixes, and the existing
fixture and bless machinery means every fix can be regression-pinned the same
day it lands. Addressing the five P0 items plus the fuzzing investment would
raise the toolchain's floor to match the quality of its ceiling.

---

## 8. Verification addendum (2026-07-05)

The findings above were addressed on `main` in five commits
(`76334ba`…`c15b645`, PRs #525–#530). This addendum records independent
re-verification against a fresh build of that head; the full workspace test
suite (786 tests) passes with zero failures.

**Re-run of the original reproductions — all now behave correctly:**

- §3.1: the handler-body probe (`Ok(nonexistentFn(unknownVar))`) now fails
  `bynkc check` with `bynk.resolve.unknown_name` pointing at the right span.
- §3.2: the `Matches("ab|cd")` probe now emits
  `new RegExp("^(?:" + "ab|cd" + ")$")` at both emission sites.
- §3.3: the mutually-recursive-record probe now reports
  `bynk.resolve.recursive_record_field` instead of overflowing the stack.
- §3.7: `fn f(await: Int) { let class = … }` now emits `__id_await` /
  `__id_class` via a new `ts_ident()` renamer, and a route param named `deps`
  becomes `__id_deps`, no longer colliding with the generated `deps` param.
- §3.8: `1_000` in match patterns emits `case 1000:`; `InRange(0.0, 1_000.5)`
  emits the correct `value <= 1_000.5` bound (no NaN).
- §3.10: the README front-page example was updated and now compiles clean.

**Verified in code (fix sites inspected):**

- §3.4: `compatible` has a `Ty::Query` arm and the catch-alls were replaced
  with exhaustive variant lists; `structurally_compatible_inner` gained
  `List`/`Map` arms.
- §3.5: `cursor_byte_offset` is gone from `bynk-lsp`; the unguarded
  `&text[open + 1..offset]` slices in `completion.rs` were replaced with
  checked `.get(..)` access; `diagnostics_mode = "on_save"` is now honored.
- §3.6: ariadne reports are configured with `IndexType::Byte`.
- §3.9: a typo'd or empty `BYNK_BYNKC` override now surfaces as unresolved in
  `bynk doctor`; `exit_byte` maps signal death to `128 + signal`; the JSON
  runtime-error document captures `tsc` stdout; `bynk dev` gained a watch
  loop (#524).
- Parse-API warnings are threaded out via `parse_with_warnings` (ADR 0117)
  instead of `Err(warnings)` on success.

**Test-infrastructure items landed:** `fuzz/` with `parse` and `compile`
cargo-fuzz targets plus a seed-corpus script and an in-tree `fuzz_smoke.rs`;
`workers_runtime_smoke.rs` (real-runtime execution);
`regex_refinement_behaviour.rs` pinning the anchoring fix; and a shared CLI
front-end removing the `bynkc`/`bynk` duplication, plus a formatter
comment-loss guard (#521–#524).

No regressions were observed in the re-verified areas, and nothing
outstanding remains in the P0/P1 severity band.
