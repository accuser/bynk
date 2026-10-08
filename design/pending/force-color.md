---
level: patch
changelog: "Rich diagnostics can be coloured without a terminal (#1777): `FORCE_COLOR` or `CLICOLOR_FORCE`, set to anything but empty or `0`, turns colour on for `less -R` or a CI log that renders ANSI. `NO_COLOR` still wins over both, as it does for clap's own help and errors"
---
