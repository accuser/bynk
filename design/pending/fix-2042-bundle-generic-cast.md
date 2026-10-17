---
level: patch
changelog: On the bundle target, a cross-context argument at a generic parameter that names a user type, such as `List[Line]` or `Map[String, Line]`, is cast to the callee's type, so a caller's own commons values pass `tsc`
---
