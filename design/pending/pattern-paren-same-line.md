---
level: patch
changelog: "A variant pattern's payload list must open on the variant's line, as a call's argument list does (#1858). A `(` on the next line no longer continues the pattern: `expect r is None` above a `()` line is a nullary pattern followed by a unit tail, not `None()`. So `bynk fmt`'s output for a comment after such a statement reparses to the same tree, and formatting it again no longer refuses with `bynk.fmt.comment_loss`. No program in the corpus wrote a pattern's `(` on a later line"
---
