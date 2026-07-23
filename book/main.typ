#import "metadata.typ": book-meta
#import "template.typ": manuscript, part-page, recto-break, rights-page, title-page

#show: manuscript.with(meta: book-meta)

#title-page(book-meta)
#rights-page(book-meta)

#counter(page).update(1)

#include "chapters/00-prologue.typ"

#part-page(1, [Saying what the system means])

#counter(heading).update(0)
#set heading(numbering: "1")

#include "chapters/01-when-architecture-becomes-convention.typ"

#include "chapters/02-a-data-shape-is-not-a-domain-model.typ"

#include "chapters/03-failure-is-part-of-the-contract.typ"

#part-page(2, [Ownership, effects, and authority])

#include "chapters/04-effects-should-name-their-requirements.typ"

#recto-break()
#include "chapters/05-state-needs-an-owner.typ"

#recto-break()
#include "chapters/06-state-changes-are-contracts.typ"

#recto-break()
#include "chapters/07-who-is-calling-is-part-of-the-operation.typ"

#recto-break()
#include "chapters/08-time-and-messages-are-architectural-boundaries.typ"
