---
level: patch
probes: diagnostic_coverage, emit_diagnostics, ts_writes
changelog: A call to a consumed context's service whose signature names a type that context consumes (`isDomestic(o: Order)`, `Order` from a third context) now checks and runs, for every caller and in `system` suites, instead of resolving the type to `()`
---
