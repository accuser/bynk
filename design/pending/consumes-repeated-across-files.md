---
level: minor
changelog: "A `consumes` clause repeated in another file of a context split across files is accepted (#1857). `consumes shop.payment as Payment` in two files of `shop.orders` was rejected as `bynk.consumes.alias_conflict`, though the unaliased repeat was accepted, and the diagnostic's \"previously defined here\" pointed at the wrong file. A repeat now counts as one clause. `alias_conflict` reports a true conflict, an alias naming two units or a unit consumed under two aliases (the latter newly rejected), with its earlier clause labelled in the same file or named in a note"
---
