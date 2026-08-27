---
level: patch
changelog: Arc F slice 3 converts `bynk-emit`'s `emit_context_deps_interface` to build a real `bynk_ts::TsDecl::Interface` instead of `writeln!`-ing text, with two fields (`surface`, `__eventsDispatch`) deliberately kept as opaque `TsType::named(...)` text — the former because its own source (`surface_ty`) is still `String`-returning, the latter because `bynk_ts::TsType::Fn` cannot represent a named parameter — zero emitted-output change.
---
