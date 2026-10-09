---
level: patch
changelog: The wire-contract hover's response set now uses the checker's recorded expression types — it matched spans including their `FileId`, which the editor's reparse never carries, so it always fell back to the declared-return guess and dropped bare variants like `NoContent`
---
