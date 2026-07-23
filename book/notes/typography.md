# Typography

The Source family and open paragraph treatment are settled as the manuscript's
typographic direction. Detailed production choices remain provisional.

## Source family

- Source Serif 4 Small Text at 10.1 pt for body copy.
- Source Serif 4 Display for book and chapter titles.
- Source Serif 4 Caption for footnotes.
- Source Sans 3 for section headings, labels, captions, and front-matter
  furniture.
- Source Code Pro at 8.2 pt for listings, inline code, and diagnostics.

Paragraphs use no first-line indent, 1.35 em of paragraph spacing, and 0.80 em
of leading. This gives the manuscript a contemporary technical-book rhythm and
clear separation between narrative and structural material.

The build uses a vendored, checksum-verified subset of static OpenType files:
Source Serif 4.005, Source Sans 3.052, and Source Code Pro 2.042. Typst is
pinned to 0.15.0 and system fonts are ignored. This makes line and page breaks
consistent between local and CI builds.

## Before production

- Revisit optical margin alignment and widow/orphan policy during copy-editing.
- Reassess the pinned font and Typst versions only as a deliberate pagination
  change, followed by a complete visual proof.

## Publication-apparatus proof

The current proof establishes a provisional page-furniture system:

- running heads use Source Sans 3 at 7.7 pt, with the book title on versos and
  the current chapter or matter title on rectos;
- a fine rule separates running heads from the text block;
- Arabic and Roman folios sit at the outside edge of the footer;
- opening, part-title, and intentionally blank pages suppress both running
  heads and visible folios while remaining part of the page count;
- the contents uses chapter-level entries only, with part entries acting as
  visual groups and subordinate chapter entries inset;
- the subject-index proof uses Source Serif 4 Small Text at 8.55 pt in two
  columns with Source Sans 3 alphabet headings.

These are proof decisions rather than settled production specifications. They
should be judged again after the contents, preface, and editorial index have
their final extent.
