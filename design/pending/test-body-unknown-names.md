---
level: minor
changelog: An unknown name or call in a test `case` or `property` body is now an error, as it is everywhere else. Test bodies were exempt, so a typo slipped through `bynkc check` unreported, and in a history property it reached test-module emission and panicked the compiler. A program that relied on the exemption no longer compiles. A rejected `for all` binding no longer echoes as an unknown name in the body (#1708)
---
