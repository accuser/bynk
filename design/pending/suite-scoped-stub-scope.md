---
level: patch
changelog: "A suite-scoped `stub` applies to its own suite's runs only (#1860). Every suite targeting a unit is emitted into one test module, and a suite-scoped clause applied to every run in it: a case in one suite ran against another suite's stub, silently. When a unit has more than one suite, each suite-scoped clause now applies only while its own suite's cases and properties run; a case-scoped clause still applies to its case alone, ahead of its suite's. A contract attack belongs to no suite, so with more than one suite it gets no suite's stub. A unit's only suite is unchanged: its clauses apply to every run, attacks included"
---
