---
level: patch
changelog: "CI checks that every name an emitted TypeScript module uses is imported or declared, without `tsc` (#1831): `bynk_ts::unbound_names` walks a module's tree, and `import_closure.rs` runs it over every module the positive corpus emits, for both targets. It found two Workers-only missing-codec imports, #1845 and #1846"
---
