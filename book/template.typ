#let accent = rgb("#4b44d6")
#let ink = rgb("#191820")
#let quiet = rgb("#686672")
#let paper = rgb("#fffefd")
#let code-paper = rgb("#f3f2f7")
#let rule = rgb("#d9d7e0")

#let body-font = "Source Serif 4 SmText"
#let body-size = 10.1pt
#let body-leading = 0.80em
#let body-spacing = 1.35em
#let display-font = "Source Serif 4 Display"
#let small-font = "Source Serif 4 Caption"
#let sans-font = "Source Sans 3"
#let mono-font = "Source Code Pro"

#let manuscript(meta: (:), body) = {
  set document(
    title: meta.title,
    author: (meta.author,),
    keywords: ("Bynk", "software architecture", "programming languages"),
  )

  set page(
    width: 7in,
    height: 9.25in,
    binding: left,
    margin: (
      inside: 0.88in,
      outside: 0.72in,
      top: 0.78in,
      bottom: 0.82in,
    ),
    fill: paper,
    numbering: "1",
    number-align: bottom + center,
  )

  set text(
    font: body-font,
    size: body-size,
    fill: ink,
    lang: "en",
    region: "GB",
    number-type: "old-style",
  )
  set par(
    justify: true,
    leading: body-leading,
    spacing: body-spacing,
    first-line-indent: 0pt,
  )

  show heading.where(level: 1): it => {
    pagebreak(weak: true, to: "odd")
    v(10%)
    set par(justify: false, leading: 1.08em)
    block(
      below: 2.8em,
      width: 100%,
    )[
      #if it.numbering != none {
        text(
          font: sans-font,
          size: 8.6pt,
          weight: "semibold",
          tracking: 0.11em,
          fill: accent,
          smallcaps[Chapter #counter(heading).display(it.numbering)],
        )
        v(0.7em)
      }
      #text(
        font: display-font,
        size: 22.5pt,
        weight: "regular",
        fill: ink,
        it.body,
      )
    ]
  }

  show heading.where(level: 2): it => block(
    above: 1.8em,
    below: 0.7em,
    text(font: sans-font, size: 15pt, weight: "semibold", it.body),
  )

  show quote.where(block: true): it => block(
    inset: (left: 1.15em),
    stroke: (left: 1.8pt + accent),
    above: 1.2em,
    below: 1.2em,
    text(size: 11pt, style: "italic", fill: rgb("#34313f"), it.body),
  )

  show raw.where(block: true): it => block(
    width: 100%,
    inset: 0.85em,
    radius: 3pt,
    fill: code-paper,
    above: 1.1em,
    below: 1.1em,
    text(font: mono-font, size: 8.2pt, it),
  )

  show raw.where(block: false): it => text(
    font: mono-font,
    size: 0.9em,
    it,
  )

  show figure.caption: it => {
    set text(font: sans-font, size: 8.8pt, fill: quiet)
    set par(justify: false, leading: 0.58em, first-line-indent: 0pt)
    it
  }

  show footnote.entry: it => {
    set text(font: small-font, size: 8.4pt)
    set par(justify: true, leading: 0.55em, first-line-indent: 0pt)
    it
  }

  body
}

#let code-listing(title, source, lang: "text") = {
  block(breakable: false, above: 1.2em, below: 1.2em)[
    #set par(justify: false, first-line-indent: 0pt)
    #text(font: sans-font, size: 8.7pt, weight: "semibold", fill: quiet)[#title]
    #v(-0.45em)
    #raw(source, lang: lang, block: true)
  ]
}

#let compiler-message(source) = block(
  width: 100%,
  inset: (left: 0.95em, right: 0.85em, y: 0.8em),
  stroke: (left: 2pt + accent),
  fill: rgb("#f7f6fa"),
  above: 1.2em,
  below: 1.2em,
)[
  #set par(justify: false, first-line-indent: 0pt)
  #text(size: 8.2pt, font: mono-font)[#source]
]

#let architecture-step(title, detail) = block(
  width: 100%,
  inset: (x: 0.65em, y: 0.7em),
  radius: 3pt,
  stroke: 0.7pt + rule,
  fill: rgb("#faf9fc"),
)[
  #set par(justify: false, first-line-indent: 0pt, leading: 0.56em)
  #text(font: sans-font, size: 9.2pt, weight: "semibold")[#title]
  #linebreak()
  #text(font: sans-font, size: 8pt, fill: quiet)[#detail]
]

#let architecture-flow() = figure(
  block(width: 100%)[
    #grid(
      columns: (1fr, auto, 1fr, auto, 1fr),
      column-gutter: 0.45em,
      align: horizon,
      architecture-step[Design decision][Orders may call payments],
      text(size: 12pt, fill: accent)[→],
      architecture-step[Representation][An import and a folder path],
      text(size: 12pt, fill: accent)[→],
      architecture-step[Enforcement][Review, tests, and memory],
    )
  ],
  caption: [How an architectural decision becomes a convention.],
)

#let title-page(meta) = page(numbering: none)[
  #set align(center)
  #set par(justify: false, leading: 1.04em)
  #set text(hyphenate: false)
  #v(19%)
  #text(font: display-font, size: 31pt, weight: "regular", fill: ink)[
    #meta.title
  ]
  #v(1.25em)
  #line(length: 2.7em, stroke: 2pt + accent)
  #v(1.45em)
  #set par(leading: 1.25em)
  #text(font: sans-font, size: 13pt, fill: quiet)[#meta.subtitle]
  #v(1fr)
  #text(font: sans-font, size: 12pt, tracking: 0.06em, smallcaps(meta.author))
  #v(8%)
]

#let part-page(number, title) = {
  set page(numbering: none)
  pagebreak(weak: true, to: "odd")
  align(center)[
    #set align(center)
    #set par(justify: false, leading: 1.08em)
    #set text(hyphenate: false)
    #v(28%)
    #text(
      font: sans-font,
      size: 9.4pt,
      weight: "semibold",
      tracking: 0.13em,
      fill: accent,
      smallcaps[Part #number],
    )
    #v(1.15em)
    #text(font: display-font, size: 26pt, weight: "regular", fill: ink)[
      #title
    ]
    #v(1fr)
  ]
  pagebreak(to: "odd")
}

#let recto-break() = {
  set page(numbering: none)
  pagebreak(weak: true, to: "odd")
}

#let rights-page(meta) = page(numbering: none)[
  #v(1fr)
  #set text(font: sans-font, size: 8.7pt, fill: quiet)
  #set par(justify: false, leading: 0.62em, first-line-indent: 0pt)
  *#meta.title* \
  #meta.subtitle

  #v(1em)
  #meta.status. Copyright (c) #meta.year #meta.author. All rights reserved.

  #v(1em)
  This is an unpublished working manuscript. The language and examples may
  change as Bynk approaches 1.0.

  #v(1em)
  Typeset with Typst.
]
