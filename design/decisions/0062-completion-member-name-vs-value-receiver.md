# 0062 — `.`-member completion splits by receiver: name now, typed value later

- **Status:** Accepted (v0.30.1)
- **Spec:** `design/karn-lsp-spec.md` §3.15
- **Refines:** ADR 0061 (completion is sliced)

## Context
ADR 0061 deferred "`.`-member + locals" to a single "slice 2". A feasibility
scout of the machinery showed that lump is really **two increments of very
different cost and risk**, split by *what sits before the dot*:

- **Name receiver** — a type/capability/sum *name* (`Color.`, `Email.`,
  `Clock.`), present verbatim in the line prefix. Members are enumerable from
  the **parsed AST** (sum variants, refined/opaque `of`/`unsafe`, capability
  ops) — no `expr_types`, no offset→type query, no scope tracking.
- **Value receiver** — a *value* binding (`list.map`, `str.split`). Needs the
  receiver's **type**, which is the hard part: `expr_types: HashMap<Span, Ty>`
  is computed during checking but **discarded on the LSP `Analyse` path**, is
  keyed by span not offset, and — fundamentally — completion fires mid-edit on
  a buffer that **doesn't parse**, so the checker never reaches the cursor
  expression. The scout's #1 risk.

## Decision
**Slice the `.`-member context by receiver.** Ship the **name-receiver**
half now (slice 2, this increment); defer the **value-receiver** half and
locals/params-in-scope to slice 3.

- Name-receiver detection is lexical and conservative: a **single
  uppercase-initial** identifier before the dot (Karn's CapitalCase-types /
  camelCase-values convention is the discriminator), excluding the decimal
  `1.` and the `.`-qualified `a.B.`. Members come from the existing
  recovery-parse walk — the same mid-edit robustness as slices 0–1.
- Receiver resolution and the members offered:
  - **sum type** → its variants;
  - **refined or opaque type** → `of`/`unsafe`. A *plain* alias `type Id = Int`
    is `Refined { refinement: None }`, and the emitter brands **every** `Refined`
    body with `of`/`unsafe` (`emit_refined_type`) — so a plain alias **does**
    carry them and they are offered (correcting the proposal's "plain alias →
    `[]`"). A **record** has no name-receiver members (fields are
    value-receiver) → `[]`.
  - **capability** → its ops (detail rendered from param *names* — a full
    Karn-syntax signature renderer isn't public cross-crate, and the op name is
    the completion's value);
  - **built-in type statics** (`Int.parse`/`Float.parse`/`Json.encode`/`decode`,
    real statics from v0.22 / ADRs 0048–0049) from a small static table, since
    they are not user-declared.

**Value `.method`/`.field` is slice 3**, ideally preceded by a design spike on
*mid-edit receiver typing*: can a receiver's type be obtained on an unparseable
buffer at all? That answer gates whether value-method completion is viable
before committing to the machinery (caching `expr_types` to the analysis, an
offset→type query, error-recovery typing). **Locals/params in scope** is also
slice 3 — no scope-at-offset query exists (the index tracks only top-level
symbols).

## Consequences
The most lexically-tractable, highest-robustness `.`-member completions land
now on the proven recovery-parse approach, with no new `karnc` machinery. The
daily-driver value-method completion — the part users will most expect — is
honestly quarantined behind the receiver-typing risk rather than half-built.
The accepted cost: a CapitalCase *value* binding (rare) is mis-offered statics
(mild noise, mirroring slice 1's record-construction note), and capability-op
details show param names without types until a renderer is shared.
