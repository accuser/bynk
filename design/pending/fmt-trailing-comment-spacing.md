---
level: patch
changelog: "`bynk fmt` no longer adds a blank line after a suite `uses` (#1808) or an agent `store` field (#1859) that carries an end-of-line comment: the comment ends the line, and the following line keeps the spacing it has without the comment. The remaining sites that ended a line with a trailing comment or a newline in two steps (an `exports` name, an `enum` variant, an actor's `auth` and `identity`) now take the one-step form too, with no change to output (#1810)"
---
