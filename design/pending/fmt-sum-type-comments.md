---
level: patch
changelog: "`fmt` keeps a `--` comment inside a sum type (#1794): above a variant, at the end of its line, on the `enum {` or `=` line, before an `enum`'s closing `}`, and between a pipe-form sum's last variant and its `embeds` clause. Such a file was refused with `bynk.fmt.comment_loss`; a commented `enum` now prints multi-line. A comment inside a variant's payload list is still refused"
---
