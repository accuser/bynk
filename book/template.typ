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

#let page-has-marker(marker) = {
  let physical = here().page()
  query(marker).any(item => item.location().page() == physical)
}

#let manuscript(meta: (:), body) = {
  let current-running-title() = {
    let matches = query(
      selector(heading.where(level: 1)).before(here(), inclusive: false),
    )
    if matches.len() == 0 {
      meta.title
    } else {
      matches.last().body
    }
  }

  let running-header = context {
    let physical = here().page()
    let is-verso = calc.rem(physical, 2) == 0
    if not page-has-marker(<plain-page>) {
      block(width: 100%)[
        #set text(
          font: sans-font,
          size: 7.7pt,
          tracking: 0.035em,
          fill: quiet,
        )
        #set par(justify: false, first-line-indent: 0pt)
        #align(if is-verso { left } else { right })[
          #if is-verso { meta.title } else { current-running-title() }
        ]
        #v(0.32em)
        #line(length: 100%, stroke: 0.45pt + rule)
      ]
    }
  }

  let running-footer = context {
    let physical = here().page()
    let is-verso = calc.rem(physical, 2) == 0
    if (
      not page-has-marker(<plain-page>)
      and here().page-numbering() != none
    ) {
      set text(font: sans-font, size: 8.2pt, fill: quiet)
      align(if is-verso { left } else { right })[
        #counter(page).display()
      ]
    }
  }

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
    header: running-header,
    header-ascent: 32%,
    footer: running-footer,
    footer-descent: 34%,
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
    if it.supplement == [Part] {
      none
    } else {
      pagebreak(weak: true, to: "odd")
      [#metadata(none) <plain-page>]
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

#let part-page(number, title) = {
  set page(header: none, footer: none)
  pagebreak(weak: true, to: "odd")
  heading(
    level: 1,
    numbering: none,
    supplement: [Part],
    outlined: true,
  )[
    Part #numbering("I", number): #title
  ]
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
      smallcaps[Part #numbering("I", number)],
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
  set page(header: none, footer: none)
  pagebreak(weak: true, to: "odd")
}

#let title-page(meta) = page(
  numbering: none,
  header: none,
  footer: none,
)[
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

#let rights-page(meta) = page(
  numbering: none,
  header: none,
  footer: none,
)[
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

#let contents-page() = {
  set outline.entry(fill: repeat([.], gap: 0.2em))
  show outline.entry: it => {
    let is-part = it.element.supplement == [Part]
    if is-part {
      block(above: 1.05em, below: 0.18em)[
        #link(it.element.location())[
          #set text(
            font: sans-font,
            size: 9.2pt,
            weight: "semibold",
            fill: accent,
          )
          #grid(
            columns: (1fr, auto),
            column-gutter: 0.65em,
            it.body(),
            it.page(),
          )
        ]
      ]
    } else {
      let inset = if it.element.numbering == none { 0pt } else { 0.95em }
      block(above: 0.3em, below: 0.3em)[
        #pad(left: inset)[
          #link(
            it.element.location(),
            it.indented(it.prefix(), it.inner(), gap: 0.65em),
          )
        ]
      ]
    }
  }
  set text(size: 9.65pt)
  set par(justify: false, leading: 0.66em, first-line-indent: 0pt)
  outline(
    title: [Contents],
    target: heading.where(level: 1),
    depth: 1,
    indent: 0pt,
  )
}

#let index-locator(target) = context {
  let matches = query(target)
  if matches.len() > 0 {
    let location = matches.first().location()
    link(location)[#counter(page).display(at: location)]
  }
}

#let apparatus-note(body) = block(
  width: 100%,
  below: 1.1em,
)[
  #set text(font: sans-font, size: 8.4pt, fill: quiet)
  #set par(justify: false, leading: 0.58em, first-line-indent: 0pt)
  #body
]

#let index-line(term, refs: (), see: none, indent: 0pt) = {
  let left = if see == none {
    term
  } else {
    [#term, _see_ #see]
  }
  let right = if refs.len() == 0 {
    []
  } else {
    refs.map(index-locator).join[, ]
  }
  block(above: 0.12em, below: 0.12em)[
    #pad(left: indent)[
      #grid(
        columns: (1fr, auto),
        column-gutter: 0.45em,
        left,
        right,
      )
    ]
  ]
}

#let subject-index(groups) = {
  let render-entry(entry) = {
    index-line(
      entry.term,
      refs: if "refs" in entry { entry.refs } else { () },
      see: if "see" in entry { entry.see } else { none },
    )
    if "subs" in entry {
      for sub in entry.subs {
        index-line(
          sub.term,
          refs: if "refs" in sub { sub.refs } else { () },
          see: if "see" in sub { sub.see } else { none },
          indent: 0.9em,
        )
      }
    }
  }

  columns(2, gutter: 1.35em)[
    #set text(font: body-font, size: 8.55pt)
    #set par(justify: false, leading: 0.58em, spacing: 0.18em)
    #for group in groups {
      if group.at("new-column", default: false) {
        colbreak()
      }
      block(breakable: false, above: 0.78em, below: 0.18em)[
        #text(
          font: sans-font,
          size: 11.5pt,
          weight: "semibold",
          fill: accent,
        )[#group.letter]
        #render-entry(group.entries.first())
      ]
      for entry in group.entries.slice(1) {
        render-entry(entry)
      }
    }
  ]
}
