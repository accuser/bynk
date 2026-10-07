---
level: patch
changelog: "Documentation truth pass (#1669). The specification now covers events (§4.1.12, §4.4.7b, §5.7b: declaration, owner-only emission, subscriptions and their filters, the envelope, schema versions, and the `Idempotency` dedup idiom) and message bundles (§4.1.13–§4.1.14, §5.11, with the `Locale` capability). Pages that still called events or storage kinds deferred now say they shipped; sagas and event replay remain planned. The 1.0 definition and design notes name the shipped agent-call form, `Counter(id).bump()`, in place of the never-built `Ref[A]`; the type-system design spec settles effect inference (declared on named declarations, inferred only on lambdas, #1529) and drops tuples (#1530)"
---
