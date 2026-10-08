---
level: patch
changelog: "`fmt` keeps an orphaned `---` doc block where it is, followed by its blank line, and formats the rest of the file, where since #1664 it refused the whole file (and format-on-save did nothing). A block before a declaration, between two, at the end of a file or body, before the unit header, or before a service's `cors`/`security`/`limits` policy is kept, and `bynk check` still warns `bynk.parse.orphan_doc_block` on the output: formatting never attaches it. A `--` comment directly under a doc block's closing `---` is no longer lost, which had made `fmt` refuse the file (#1756)"
---
