---
level: patch
changelog: "`emit_composition_root`'s event-fanout envelope-forwarding decision (`wants_envelope`) now reads a project-wide `EventSubscriberShape` accumulator, populated at each unit's own check time, instead of reaching into a different, already-consumed unit's raw `UnitTable` directly (#1232)"
---
