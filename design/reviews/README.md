# Reviews

A review is a **dated, point-in-time snapshot** — an architecture or
code-quality assessment against a specific commit or version — not a standing
document. Unlike a [track](../tracks/README.md), a review is never updated in
place; its findings age as the tree moves, and a document that cites one
re-measures against the current tree rather than trusting the review to still
be current (see, for example, `../bynk-compiler-trajectory.md` §3.0: "every
probe below was run against the working tree, not taken from the July
review").

Named `YYYY-MM-DD-<slug>.md`. A review has no lifecycle of its own — it is
evidence, cited by whatever track, ADR, or proposal it informed, and kept for
the record once superseded rather than deleted.
