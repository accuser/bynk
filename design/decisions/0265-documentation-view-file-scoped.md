# 0265 — The documentation view is file-scoped in Tier 1

- **Status:** Accepted (v0.225)

**Context.** A `context` is the natural documentation unit — it is the module,
possibly spanning several files. But the per-file `document_symbols` walk
already exists and is the trivial Tier-1 build; context-aggregation (merging
every file of a multi-file `context`) needs a new cross-file assembly with no
precedent for this query.

**Decision.** Ship **file-scoped**. `bynk/documentationModel` builds its model
from a single committed snapshot's text — exactly like `document_symbols` and
`bynk/sequenceModel` — with no cursor position (the page is the whole file, so
the params are a bare `textDocument`). `bynk_ide::documentation::documentation_model`
walks the parsed unit's items with an *exhaustive* match on `CommonsItem`, so a
new declaration kind is a compile error, not a silently-missed page row.

**Consequences.** A multi-file `context` documents one file at a time.
Context-aggregation is the deferred follow-up. A `suite` unit has no doc page
(its `case`/`stub` members have no `describe_*` renderer) and returns empty,
on the same terms as a non-project file or a file with no committed round.
