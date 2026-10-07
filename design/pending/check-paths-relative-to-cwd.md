---
level: patch
changelog: "`bynkc check` and `bynk check` (and a failing `bynkc test` build) name each file by the path you'd type from the working directory, the same path `fmt` reports: `bynkc check test/fixtures/x` reports `test/fixtures/x/src/greeting.bynk:6:36`, where it used to report `src/greeting.bynk:6:36`. Problem matchers that resolve against the directory the command ran in (bynk-ci's, VS Code's) now find the file. This changes the output format for anything parsing these paths (#1772)"
---
