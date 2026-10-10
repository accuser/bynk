# 0446 — A consumer generates a consumed context's unexported boundary types' codecs under a qualified name

- **Status:** Accepted (v0.315.1)

**Context.** A consumer decodes a consumed context's exports with codecs it generates locally (#661): the codec function names stay bare, and their TS types reach through the callee's type-only namespace (`import type * as t_vault`). Only the callee's *exported* types were generated. A type an export reaches without being exported, like `Ticket.code: Code`, was classed as foreign and expected from a commons import, which never applies to it. So the consumer called a codec that didn't exist (#1846). The consumer can't name that type, so it may declare its own `Code`. Generating the callee's codec under the bare name would then collide with the consumer's, or the dedupe against `emitted_names` would silently decode the callee's `Code` with the consumer's codec.

**Decision.** In its copy of the callee's type table, the consumer renames each type the callee declares but doesn't export to `<ns>__<Type>` (`t_vault__Code`), both where it is declared and wherever it is named. The rename is in `emit_consumed_context_helpers`, via `bynk_check::wire::subst_type_ref`. Codec and instantiation names follow from the type name, so they come out distinct with no change to the codec generators.

A type qualifier (`Qual`) entry may now be a whole name with no trailing `.` (`t_vault.Code`) as well as a namespace prefix. That keeps the TS type the callee's own and keeps `Provenance::Consumed`, so a refined type is validated inline (Decision D), reporting its declared name. A commons type the callee `uses` and the consumer reaches as the same declaration keeps its name and is imported from the commons, as before.

Importing the codec from the callee's `handlers.ts` was rejected, because it would link one Worker's code into another.

**Consequences.** Only Workers output changes, and only for a consumer whose decoded exports reach an unexported type. A consumer may carry two codecs for two types that share a name in the source. That is deliberate: they are distinct types with distinct validation.
