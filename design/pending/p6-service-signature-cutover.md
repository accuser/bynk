---
level: patch
changelog: "emit_service now reads a handler's resolved signature (params/return type/effectful-ness) and a service's protocol data from bynk-emit::ir instead of walking bynk_syntax::ast TypeRefs/ServiceProtocol directly (internal only, byte-identical output — no language surface change)"
---
