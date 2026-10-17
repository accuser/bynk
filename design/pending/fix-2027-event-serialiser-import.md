---
level: patch
probes: ts_writes
changelog: On Workers, a context's codec for a list, option, map or generic record of a commons type names the type unbranded (`__CommonsLine`), so a subscriber's codec for an event like `{ lines: List[Line] }`, or a caller's codec for a consumed record with such a field, accepts the other context's `Line` and passes `tsc`
---
