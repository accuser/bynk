---
level: patch
changelog: "A context now imports every type a codec it emits names, including a used commons' field type its own code never reads (#1829). A context whose agent stores, or whose code `Json.encode`s, a used commons' `Item` emitted `__serialise_Note(value: Note)` for the unread `item.note` without importing `Note`, and a file of a context split across files did the same for a sibling file's type, so `tsc` failed with TS2304 (under `bynkc test` and `bynk dev`, and on Workers for `Json`). Each type a module's boundary or `Json` codec helpers name is now an implied import, kept when the emitted helpers spell it"
---
