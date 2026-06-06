# How these docs are organised

This book follows **[Diátaxis](https://diataxis.fr/)** — a framework that splits
technical documentation into four kinds, because a reader *learning* a language
needs something very different from a reader *looking up* an exact rule. Each
kind has its own voice and its own page. They are deliberately **not mixed**: a
tutorial will not stop to argue design rationale, and a reference page will not
walk you through a task.

Knowing which of the four you are in tells you what to expect — and where to go
when a page is not what you need.

## The four kinds

| Kind | When you are… | It answers | What it looks like |
|---|---|---|---|
| **[Tutorials](../tutorials/01-first-program.md)** | learning | “Teach me.” | A guided lesson. The author drives; it is guaranteed to work end to end. |
| **[How-to guides](../how-to/index.md)** | doing a task | “How do I X?” | Steps to a goal you already have. Assumes you know the basics. |
| **[Reference](../reference/index.md)** | looking something up | “What is the exact behaviour of X?” | Dry, complete, accurate. Structured like the language itself. |
| **[Explanation](../explanation/index.md)** | trying to understand | “Why is it like this?” | Discussion, rationale, and trade-offs. |

Two axes underlie the table. **Tutorials** and **how-to guides** are for
*action* (doing); **reference** and **explanation** are for *cognition*
(thinking). **Tutorials** and **explanation** serve *acquiring* skill and
understanding; **how-to guides** and **reference** serve *applying* what you
already have.

```
                 ACQUISITION                 APPLICATION
            (learning / studying)       (working / applying)
          ┌───────────────────────┬───────────────────────┐
   ACTION │       Tutorials       │     How-to guides      │
 (doing)  │   "teach me"          │   "how do I X?"        │
          ├───────────────────────┼───────────────────────┤
COGNITION │     Explanation       │      Reference         │
(thinking)│   "why is it so?"     │   "what exactly is X?" │
          └───────────────────────┴───────────────────────┘
```

## How to use them together

The kinds link *outward* to one another rather than repeating content, so each
page can stay short:

- A **how-to guide** points to the **reference** for exact rules and to an
  **explanation** for the reasoning — it does not reproduce either.
- A **tutorial** gets you to a working result; when you want to know *why* a
  step works, it sends you to **explanation**.

So: follow a **tutorial** when you are new, grab a **how-to** when you have a
job to do, consult the **reference** to confirm exact behaviour, and read an
**explanation** when you want the reasoning behind a decision.

## A note on the audience layout

The four kinds above all serve **language users** — people writing Karn. The
book reserves two further top-level sections that will be filled in later:

- **[Contributing to the compiler](../contributing/index.md)** — for people
  working on `karnc` itself.
- **[Tooling](../tooling/index.md)** — for `karn-fmt`, `karn-lsp`,
  `tree-sitter-karn`, and the VS Code extension.

Each audience gets its own coherent set of the four Diátaxis kinds, so the
modes stay unmixed within a clear audience.

## A note on status

Karn is pre-1.0 and changes in small increments. Pages document **what compiles
today**. Anything still on the roadmap is marked as planned, and pages not yet
written are flagged _“To be written.”_ See
[Versioning & roadmap](../explanation/versioning-and-roadmap.md) for how the
book tracks the language.
