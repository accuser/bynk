---
level: patch
changelog: "Rich diagnostics are coloured only when stderr is a terminal and `NO_COLOR` is unset or empty, so a piped or redirected report has no ANSI escapes. A source line longer than 400 bytes is cut to a window around its labels, marked `…`, with the header still naming the column in the file: a diagnostic on a megabyte line used to take seconds and write megabytes, five escapes a character (#1666)"
---
