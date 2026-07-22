#import "metadata.typ": book-meta
#import "template.typ": manuscript, rights-page, title-page

#show: manuscript.with(meta: book-meta)

#title-page(book-meta)
#rights-page(book-meta)

#counter(page).update(1)

#include "chapters/00-prologue.typ"

#counter(heading).update(0)
#set heading(numbering: "1")

#include "chapters/01-when-architecture-becomes-convention.typ"

#include "chapters/02-a-data-shape-is-not-a-domain-model.typ"

#include "chapters/03-failure-is-part-of-the-contract.typ"
