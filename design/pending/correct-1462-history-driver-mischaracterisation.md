---
level: patch
changelog: Corrects #1462's own track-doc claim that `emit_agent`'s history-driver (`__bynkDriveHistory_*`) is "a real, tractable, previously-uncounted conversion candidate" — #1386 already decided, before #1462 landed, that this exact function is a deliberate, standing exclusion (test-support-only, thinnest fixture coverage in the track, its own real algebra question deferred to `any` by design regardless of conversion). The history-driver moves from the "argued, not yet attempted" bucket to the permanent one, alongside `lower.rs` and the Decision-C wrapper text. No code change.
---
