# 0323 — R2.3's ambient-filesystem ban is scoped to file contents, not directory enumeration

- **Status:** Accepted (v0.247.13)

**Context.** R2.3 (`design/bynk-greenfield-compiler.md`) says "`Sources` is
constructed once, at the process edge, and is the compiler's only view of
file *contents*." Two functions in `bynk-emit/src/project/discovery.rs` read
the filesystem directly: `read_source` (content, overlay-first with a disk
fallback) and `discover_bynk_files` (a bare `fs::read_dir` walk with no
overlay parameter, finding which `.bynk` files exist). The content-ownership
track (#1086) needed to decide whether R2.3 covers both or content only,
since it determines whether `bynk-emit`'s `fs_below_driver` count can reach 0
or has a permanent, named floor.

**Decision.** Narrow. R2.3's own wording is content-scoped; `discover_bynk_files`'s
enumeration walk is not in this track's scope and stays below the driver.
The alternative — treating `bynk-lsp`'s already-shipped
`didChangeWatchedFiles` watcher (`bynk-lsp/src/lib.rs:1260`,
`lsp-foundations.md` slice E) as a maintained enumeration index instead of an
invalidation signal — was considered and rejected for this track: checked
against its actual implementation (`did_change_watched_files`,
`bynk-lsp/src/lib.rs:3378`), the watcher reacts to file events by
re-scheduling analysis rounds; it does not itself hold an enumerated file
list. Building that index (its own initial-sweep-vs-first-event race, and a
fallback story for a client that doesn't support dynamic watcher
registration) is a real, independently-scoped subsystem, not a side effect
of this track's slice 6.

**Consequences.** `design/greenfield-status-rules.md`'s `fs_below_driver`
probe keeps flagging `bynk-emit=2` (`discovery.rs`'s walk, and `project.rs`'s
`use std::fs;` serving it) after this track's last slice ships, until a
follow-up amends the probe to stop counting pure-enumeration functions —
named here as a real follow-on, not silently absorbed into a false "0". A
future track wanting enumeration-side freshness (an unsaved rename/create
visible without a re-scan) can pick this back up with its own design pass;
this decision does not foreclose it, it just declines to bundle it into
#1086.
