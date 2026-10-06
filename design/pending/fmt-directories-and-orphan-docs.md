---
level: minor
changelog: "`bynk fmt` and `bynkc fmt` accept directories: a project root formats the files `check` reads (its `[paths] include` trees minus `exclude`), and any other directory is walked recursively, so `fmt --check .` covers what `check .` and `test .` see (#1753). The formatter no longer deletes a `---` documentation block that attaches to no declaration (one separated from the next declaration by a blank line, or at the end of a file): it leaves the file unchanged and reports `bynk.fmt.comment_loss`, as it does for a comment it cannot place (#1664)"
---
