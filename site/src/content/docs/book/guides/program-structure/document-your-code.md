---
title: Document your code
---
**Goal:** write documentation that the tooling shows wherever a name appears.

Bynk has two kinds of comment, and they do different jobs:

- A **line comment**, `-- …`, is a note for whoever edits the code. It runs to
  the end of the line, and the tools ignore it.
- A **doc-block**, `--- … ---`, is documentation. It attaches to the
  declaration that follows it. Hover shows it, and so does the editor's
  **Bynk: Show Documentation** page.

Use `--` for remarks about how the code works, and `---` for what a reader of
the API needs to know.

## Document a declaration

Put a doc-block directly above the declaration. The markers are lines of dashes
on their own, and everything between them is the doc:

```bynk
---
A repository, as `owner/name`.
---
type Repo = String

---
The number of open issues in `repo`, or zero when it has none.
---
fn openIssues(repo: Repo) -> Int { 0 }
```

Any declaration can carry one: a type, a function, a capability and its
operations, a service, an agent and their handlers, an actor, an event, a
message bundle. Hover on `Repo` anywhere it is used, and you see its signature
with this doc below it.

## Document a module

A doc-block **above the unit header** documents the unit itself. This is its
*module doc*:

```bynk
---
The report the org canary posts, and how versions are ordered.
---
commons compat.model

---
A repository, as `owner/name`.
---
type Repo = String
```

The module doc shows when you hover the unit's name: in its own header, in a
`uses compat.model`, or in a `consumes` of a context. It is also the opening
paragraph of the unit's **Show Documentation** page.

### Turn a header comment into a module doc

A file often opens with a `--` comment that says what the module is for. The
[compat-board](https://github.com/bynk-lang/compat-board) project's files began
that way:

```bynk
commons compat.model

-- The report the org canary posts, and how versions are ordered.

type Repo = String
```

That comment is invisible to the tools. To make it the module doc, move it above
the header and turn it into a doc-block, as in the example above. The text says
what the module is, so it belongs in the module doc. Keep a `--` comment for a
note that only an editor of the file needs.

### One module doc per unit

A context (or a commons) can be split across several files in its directory,
each opening with the same header. **At most one of those files may carry the
module doc.** A unit's doc is one piece of prose. If two files each had one, the
tools would have to join them in some file order, and the result would read
oddly. So two or more is the error `bynk.project.duplicate_module_doc`, which
names every file that carries one.

Pick the file that best introduces the unit, and keep the doc there:

```bynk
---
The shop: carts and checkout.
---
context shop

type CartId = String
```

Every other file of `context shop` opens with the bare header, or a `--`
comment above it:

```bynk
-- Payment handling for the shop.
context shop

type Amount = Int
```

Hover and the documentation page find the doc in whichever file has it. The
page for any file of `shop` shows the whole context: the one module doc, then
every file's declarations.

A `suite` keeps its own doc, which describes the suite, so this rule doesn't
apply to it.

## Write Markdown in a doc

A doc is Markdown. Headings, lists, emphasis, links and code all render on
hover and on the documentation page. A `[Name]` link to another declaration
becomes a link you can follow.

To show an example in a doc, put it in a code fence. Prefer `~~~` fences, which
read clearly inside Bynk source:

```bynk
---
The major part of a release version. For example, for

~~~text
0.309.4
~~~

it is `0`.
---
fn major(version: String) -> Int { 0 }
```

Keep code in a fence. Bynk code outside a fence in a doc-block reads as code
that was turned into documentation by mistake.

A line of dashes on its own is a doc-block **marker**, never a divider. So a
doc that needs a `---` line of its own, such as a Markdown horizontal rule or a
Bynk example with its own doc-block, must open with a marker that line cannot
close. [Spec §3.3.2](/book/spec/lexical-grammar/#332-doc-blocks) gives the exact
rule for which marker closes a block.

## `--` or `---`?

| You want to… | Write |
|---|---|
| Explain what a declaration or a unit is, for its users | a `---` doc-block above it |
| Leave a note for someone editing the code | a `--` line comment |
| Separate sections of a file | a `--` comment with text, such as `-- Helpers --` |
| Disable code temporarily | `--` at the start of each line |

Don't use a line of dashes as a divider. It opens a doc-block, and the code
below it becomes documentation.

**See also:** [Spec §3.3 — Comments and
doc-blocks](/book/spec/lexical-grammar/#33-comments-and-doc-blocks),
[Spec §5.1a — Module documentation](/book/spec/static-semantics/#module-docs).
