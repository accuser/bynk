---
level: patch
changelog: "A unit that `uses` a commons now imports every type of that commons its emitted TypeScript names, not only the ones its source spells (#1778). A literal admitted as a refined type is cast to it (`(\"a\" as Repo)`), and a list kernel annotates its callback with the element type (`(__x: Repo) => …`), so a record literal or a mapped field whose type was declared in a used commons failed `tsc` with TS2304, in a context and in a second commons alike. `bynkc check` passed while `bynkc test`, `bynk dev` and `bynk deploy` failed. A context imports the extra types aliased and rebranded, like any other `uses` type"
---
