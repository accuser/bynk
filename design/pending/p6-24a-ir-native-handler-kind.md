---
level: patch
changelog: "P6.24a: `IrHandler::kind` is now a real IR-native mirror (`IrHandlerKind`/`IrHttpMethod`), not `bynk_syntax::ast::HandlerKind` reused verbatim; `emitter.rs`'s five purely-structural handler-kind/protocol header-import checks (`has_http`, `has_queue`, `hosts_ws_open`, `hosts_ws_inbound`, `subscribed_event_type_names`) now read it and the existing `ProtocolIr` (via a new `TypedCommons`-only `lower_protocol_ir_from_commons` sibling) instead of matching raw `HandlerKind`/`ServiceProtocol` directly — byte-identical output confirmed by zero-diff bless"
---
