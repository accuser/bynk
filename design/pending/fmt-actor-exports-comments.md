---
level: patch
changelog: "`fmt` keeps `--` comments inside an `actor { … }` body and an `exports … { … }` list (#1797): on the `{` line, above an `auth`/`identity` entry or an exported name, at the end of its line, and before the `}`. A commented actor or list prints in its multi-line form. These used to be refused with `bynk.fmt.comment_loss` wherever they sat. A comment inside an `if`/`else` block is still refused (#523)"
---
