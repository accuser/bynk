---
level: patch
changelog: The language server no longer fails silently. A panic during project analysis is logged and shown once in the editor (its diagnostics may be stale until a later clean round), where it used to be dropped. A client message that isn't valid JSON is logged and skipped, and a lone UTF-16 surrogate escape is repaired to U+FFFD, where either used to end the session with exit 0. The server now exits 1, with the reason on stderr, when a session ends without `shutdown` (#1667)
---
