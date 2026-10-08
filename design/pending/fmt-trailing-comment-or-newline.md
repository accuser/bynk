---
level: patch
changelog: "`bynk fmt` output is unchanged (internal; suggested by the review of [#1799](https://github.com/accuser/bynk/pull/1799)). The formatter ends a line, with a trailing comment or a bare newline, through one helper rather than a two-statement idiom repeated at 27 sites, where writing one half without the other was an easy mistake"
---
