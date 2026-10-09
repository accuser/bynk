---
level: patch
changelog: "A context whose agent stores a used commons' record now imports every type that record reaches, including a field type the context's own code never reads (#1829). The agent-state codec the context emits names that type (`__serialise_Note(value: Note)`), but the import set came only from what the source mentions, so `bynkc test` and `bynk dev` builds failed `tsc` with TS2304. Each type a module's boundary helpers name is now an implied import, kept when the emitted helpers spell it"
---
