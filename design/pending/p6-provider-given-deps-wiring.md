---
level: patch
changelog: "project.rs's instantiate_provider_expr now reads a provider's given clause from bynk-emit::ir (CapRefIr) instead of walking bynk_syntax::ast::CapRef directly; ProviderBody::External also gains the given field it was silently dropping (internal only, byte-identical output)"
---
