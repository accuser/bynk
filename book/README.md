# Bynk print manuscript

This directory contains the source of a narrative, print-first book about the
problems Bynk is designed to address and the choices it makes in addressing
them.

It is deliberately separate from the online Bynk Book in `site/`:

- the online Book teaches and documents Bynk;
- this manuscript develops an argument about service design, with Bynk as its
  worked answer;
- documentation may inform the manuscript, but prose is not imported or shared
  mechanically between them.

The title, subtitle, structure, trim, and component vocabulary are provisional
while the manuscript finds its shape.

## Source structure

- `main.typ` assembles the manuscript.
- `metadata.typ` holds working publication metadata.
- `template.typ` owns page design and semantic presentation rules.
- `frontmatter/`, `chapters/`, and `backmatter/` contain publishable text.
- `notes/` contains editorial planning and source maps, not manuscript prose.
- `snippets/` contains book-specific Bynk programs and fragments.
- `figures/` contains original book artwork.
- `build/` is ignored local output.

Chapter files should describe meaning, not page geometry. New visual components
belong in `template.typ`; they should only be added when real manuscript content
demands them.

## Toolchain

The current specimen targets **Typst 0.15.0**. Build from the repository root so
future listings can read compiler-tested examples elsewhere in the repository:

```sh
mkdir -p output/pdf
typst compile --root . \
  --font-path /path/to/source-fonts \
  book/main.typ output/pdf/bynk-manuscript.pdf
```

For continuous preview while writing:

```sh
typst watch --root . \
  --font-path /path/to/source-fonts \
  book/main.typ output/pdf/bynk-manuscript.pdf
```

The generated PDF is not committed.

### Source fonts

The manuscript uses Source Serif 4 Small Text for narrative text, Source Serif
4 Display for chapter and book titles, Source Serif 4 Caption for footnotes,
Source Sans 3 for section headings and book furniture, and Source Code Pro for
listings, inline code, and diagnostics. There is no alternate typography
setting.

The Source fonts are distributed by Adobe under the SIL Open Font License 1.1.
They are not currently vendored; exact static or variable files and a
reproducible acquisition method must be settled before production.

## Working principles

1. Begin with the engineering problem; introduce Bynk as a response.
2. Use code as evidence, not as a disguised reference manual.
3. Show compiler refusals where they reveal the language's design.
4. State costs and counterarguments alongside benefits.
5. Prefer one evolving system over disconnected feature demonstrations.
6. Keep exact syntax and exhaustive reference material in the online Book.
7. Compile-test every listing presented as a complete program.
8. Use sentence case for book, part, chapter, and section titles.

See `notes/brief.md` for the current editorial proposition,
`notes/source-map.md` for the boundary between research material and manuscript
prose, and `notes/typography.md` for the current typographic specification.
