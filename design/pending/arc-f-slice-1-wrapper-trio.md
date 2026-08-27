---
level: patch
changelog: Arc F slice 1 converts `bynk-emit`'s `emit_helpers_for_owner[_qualified]`/`emit_generic_helpers[_qualified]` (and `emit_one`) to return real `Vec<TsDecl>` trees instead of `writeln!`-ing into `out: &mut String`, with the boundary-print step consolidated into one shared `print_decls` helper at every caller — zero emitted-output change.
---
