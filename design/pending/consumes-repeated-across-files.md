---
level: minor
changelog: "A `consumes` clause repeated in another file of a context split across files is accepted (#1857), aliased (`consumes shop.payment as Payment`) or as a `{ Cap }` selection; both were rejected, though the unaliased repeat was accepted, and the alias diagnostic's \"previously defined here\" pointed at the wrong file. The same clause twice in one file is still an error. `alias_conflict` reports a true conflict, an alias naming two units or a unit consumed under two aliases (the latter newly rejected), labelling the earlier clause in the same file or naming its file in the message"
---
