---
level: minor
changelog: A doc block containing `*/` can no longer terminate the emitted JSDoc comment early and inject top-level TypeScript
---

## ADR: doc-block-comment-escape
title: Escape `*/` in emitted doc-block comments
summary: A doc block's `*/` is neutralised so it cannot close the JSDoc comment and inject module-scope code

**Context.** Doc-block bodies are lexed verbatim from the source text between
`---` markers (`bynk-syntax/src/lexer.rs`, `doc_block_content`): the lexer only
treats `---` lines as delimiters, so every other byte — including a literal
`*/` — passes through unaltered. The emitter's `emit_doc_block`
(`bynk-emit/src/emitter/emit.rs`) then wrote each line verbatim inside a
`/** … */` JSDoc comment. Doc blocks attach to top-level `type`/`fn`/service/
agent declarations, so the comment sits at TypeScript module scope.

A `*/` in the body therefore closed the JSDoc comment early, and whatever
followed on the line landed as executable top-level TypeScript. A source doc
block of `docs */ ; (globalThis as any).PWNED = true; /*` emitted a `*/` that
closed the comment, a statement that ran at module load, and a trailing `/*`
that swallowed the emitter's own ` */` — a verified end-to-end injection from
`.bynk` source into generated code (#720).

**Decision.** `emit_doc_block` escapes every `*/` in a doc line to `*\/` before
writing it. In a block comment `*\/` cannot terminate the comment — the `*` is
no longer immediately followed by `/` — while rendering identically to a reader.
The escape is applied per line, so the only `*/` the emitter can produce is the
single closer it writes itself. No lexer or parser change is made: the doc body
is still captured verbatim, and the neutralisation lives at the one emission
site that embeds it in a comment.

**Consequences.** A doc block can no longer alter the structure of the emitted
module or introduce top-level code, regardless of its content. The change is
confined to `emit_doc_block`; every doc-block call site (types, refined/record/
sum decls, free functions and methods, capabilities, providers, services,
contexts, handlers, agents) inherits it. Doc blocks that never contained `*/`
emit byte-for-byte as before. The only observable difference is cosmetic: a
literal `*/` in documentation now renders as `*\/` in the generated JSDoc.
