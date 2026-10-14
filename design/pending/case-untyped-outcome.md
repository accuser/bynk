---
level: patch
changelog: 'In a test case, matching or testing a service call''s outcome binds the fixed type of a variant''s payload (`Raw(body, contentType)` binds `Bytes` and `String`, a message or redirect variant `String`), so its methods work. The match''s arms are now checked, and a method on a payload of the handler''s result type is `bynk.test.untyped_outcome` instead of TypeScript that failed `tsc` (#1931)'
---
