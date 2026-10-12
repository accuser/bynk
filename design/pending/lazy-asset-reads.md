---
level: patch
changelog: 'A build reads only the `assets/` files an `asset` declaration names, so large undeclared media there no longer slow `bynkc compile` or each `bynk dev` rebuild; a declared file that cannot be read is the new `bynk.asset.unreadable_file` (static assets, #1883)'
---
