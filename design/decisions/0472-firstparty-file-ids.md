# 0472 — First-party units carry reserved `FileId`s and their labels render against their own source

- **Status:** Accepted (v0.332.3)

**Context.** Secondary labels are attributed to a file by the `FileId` their
span carries (#1659), and a label whose span has no identity
(`FileId::UNKNOWN`) is taken to be in the error's own file. First-party units
(`bynk.bynk`, `bynk/cloudflare.bynk`, …) were lexed with no identity, so a
label pointing at a declaration in one — `bynk.resolve.missing_field`'s "field
declared here" for `Request.headers` — was drawn at the same byte offsets in
the user's file (#1956). Their parse is cached once per process, so the id
cannot come from any one project's `parse_cache` interner.

**Decision.** Each first-party unit's spans carry a fixed, reserved `FileId`,
`FIRSTPARTY_FILE_ID_BASE + id_block` (`0xFFFF_0000` + its block), mirroring the
fixed `ExprId` block the same parse already reserves. Label attribution maps
such an id to `LabelFile::FirstParty { identity_path, source }`, carrying the
unit's own text because it is not a project file and is never in the build's
snapshots. The CLI draws the label in a snippet of that source, headed by the
bare identity path (`bynk.bynk:46:3`); the LSP, which has no document to point
at, makes it a `note:`. The checker's error sink does not register first-party
ids, so which file an error itself is attributed to is unchanged.

Rendering hands ariadne char offsets (`IndexType::Char`), converted per label
against that label's own source, rather than byte offsets: ariadne 0.6's byte
mode heads a non-primary file's snippet by looking the label's char offset up as
a byte offset, which names the wrong line after non-ASCII text and panics when
the offset splits a codepoint — and the standard library's prose is full of
`—`.

**Consequences.** A cross-file label into the standard library shows the actual
declaration. Any further first-party unit gets its id from its `id_block` with
no other change. Real-file ids from `parse_cache::file_id_for` count up from 0
and would need four billion files to reach the reserved range.
