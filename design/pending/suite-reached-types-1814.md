---
level: patch
changelog: "A test suite checks a type its target reaches only through an imported declaration (#1814). A unit suite's view and a `system` case's harness table each merged one level of `uses`, so `t.model`'s `Run { repo: Repo }` reached a suite over `t.web` (which `uses t.model` only) with `repo` typed by nothing: `Run { repo: 42 }` and an `Int` read of `r.repo` compiled. Each suite view is now closed over the types its imported declarations reach, as the unit's own table is (#1807), and the same naming gate rejects a reached type a suite writes. The generated test module imports each reached type from its owning commons, so a lowered case can spell it (`(__x: Repo) => …`, `Repo.shout(…)`)"
---
