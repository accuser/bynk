//! Strip-only TypeScript → JavaScript for Bynk's first-class JS artefact (the
//! in-browser track, slice 1 — ADR 0137).
//!
//! `bynkc` emits TypeScript. A JS artefact is *emit-then-strip*: the same emitter
//! output with type annotations erased and nothing else changed. Because the
//! emitter is **strip-only** (ADR 0136 — every emitted `.ts` is erasable by pure
//! type-stripping), the transform here is total and lossless for runtime
//! behaviour: it never has to lower a type-directed construct (parameter
//! property, `enum`, `namespace`), only delete type syntax.
//!
//! The engine is [`oxc`] — a pure-Rust TS parser, type-erasing transform, and
//! codegen — so neither `bynkc --emit js` nor the in-browser compile path has any
//! Node/`tsc` dependency, and the crate compiles to `wasm32` for the playground.
//!
//! The transform is configured for **pure type-stripping**, matching Node's
//! `stripTypeScriptTypes` (the slice-0 strip oracle): `only_remove_type_imports`
//! keeps every *value* import even when unused, eliding only `import type` and
//! `type` specifiers — TypeScript's import-elision-by-usage is deliberately off,
//! so stripping is a syntactic erase, not a semantics-aware rewrite.

use std::fmt;
use std::path::Path;

use oxc::allocator::Allocator;
use oxc::codegen::Codegen;
use oxc::parser::Parser;
use oxc::semantic::SemanticBuilder;
use oxc::span::SourceType;
use oxc::transformer::{TransformOptions, Transformer, TypeScriptOptions};

/// A failure to strip TypeScript to JavaScript. For input produced by the Bynk
/// emitter this should never occur — the emitter only emits valid, strip-only
/// TypeScript (ADR 0136) — so a `StripError` indicates an emitter or toolchain
/// bug rather than user error.
#[derive(Debug, Clone)]
pub struct StripError {
    /// The file being stripped (for diagnostics).
    pub filename: String,
    /// What went wrong (parse or transform diagnostics).
    pub message: String,
}

impl fmt::Display for StripError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "failed to strip types from {}: {}",
            self.filename, self.message
        )
    }
}

impl std::error::Error for StripError {}

/// Strip TypeScript types from `source`, returning equivalent JavaScript.
///
/// `filename` selects the source flavour (`.ts`/`.tsx`/`.mts`) and labels
/// diagnostics; it does not have to exist on disk. Value imports are preserved
/// verbatim (see the module docs); only type syntax is erased.
pub fn strip_types(source: &str, filename: &str) -> Result<String, StripError> {
    let allocator = Allocator::default();
    let source_type = SourceType::from_path(filename).unwrap_or_else(|_| SourceType::ts());

    let parsed = Parser::new(&allocator, source, source_type).parse();
    if parsed.panicked || !parsed.diagnostics.is_empty() {
        return Err(StripError {
            filename: filename.to_string(),
            message: format!("parse error: {}", join_diagnostics(&parsed.diagnostics)),
        });
    }

    let mut program = parsed.program;
    // `with_enum_eval(true)`: the transformer *panics* on an `enum` without it.
    // Strip-only emitter output never contains one (ADR 0136), but evaluating
    // enums keeps this a total function — graceful transform, never a panic — if
    // a non-strip-only source is ever handed in.
    let scoping = SemanticBuilder::new()
        .with_enum_eval(true)
        .build(&program)
        .semantic
        .into_scoping();

    let options = TransformOptions {
        typescript: TypeScriptOptions {
            // Pure type-stripping: keep every value import (even unused),
            // erase only `import type` / `type` specifiers. Matches Node's
            // strip-only mode rather than TypeScript's usage-based elision.
            only_remove_type_imports: true,
            ..TypeScriptOptions::default()
        },
        ..TransformOptions::default()
    };

    let ret = Transformer::new(&allocator, Path::new(filename), &options)
        .build_with_scoping(scoping, &mut program);
    if !ret.diagnostics.is_empty() {
        return Err(StripError {
            filename: filename.to_string(),
            message: format!("transform error: {}", join_diagnostics(&ret.diagnostics)),
        });
    }

    Ok(Codegen::new().build(&program).code)
}

fn join_diagnostics(diags: &[oxc::diagnostics::OxcDiagnostic]) -> String {
    diags
        .iter()
        .map(|d| d.to_string())
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Assert the stripped JS contains `needle` and none of the `absent` strings.
    fn strip(src: &str) -> String {
        strip_types(src, "test.ts").expect("strip should succeed on valid strip-only TS")
    }

    #[test]
    fn erases_annotations_and_keeps_values() {
        let js = strip("export const add = (a: number, b: number): number => a + b;\n");
        assert!(js.contains("export const add"));
        assert!(!js.contains(": number"), "annotations erased:\n{js}");
    }

    #[test]
    fn removes_type_aliases_and_interfaces() {
        let js = strip(
            "export type Id = string & { readonly __brand: \"x\" };\n\
             export interface Logger { info(m: string): Promise<void>; }\n\
             export const v = 1;\n",
        );
        assert!(!js.contains("interface"), "interface erased:\n{js}");
        assert!(!js.contains("type Id"), "type alias erased:\n{js}");
        assert!(js.contains("export const v = 1"));
    }

    #[test]
    fn preserves_value_imports_drops_type_specifiers() {
        // `Ok`/`Err` are value imports and must survive even though they are
        // unused here; `type Result`/`import type` must go.
        let js = strip(
            "import { Ok, Err, type Result } from \"./runtime.js\";\n\
             import type { Foo } from \"./foo.js\";\n\
             export const x = 1;\n",
        );
        assert!(js.contains("Ok"), "value import Ok kept:\n{js}");
        assert!(js.contains("Err"), "value import Err kept:\n{js}");
        assert!(!js.contains("Result"), "type specifier dropped:\n{js}");
        assert!(!js.contains("Foo"), "import type dropped:\n{js}");
        assert!(
            !js.contains("./foo.js"),
            "type-only import line dropped:\n{js}"
        );
    }

    #[test]
    fn de_sugared_provider_constructor_strips() {
        // The shape the slice-0 emitter produces for a `given` provider.
        let js = strip(
            "export class P {\n\
             \x20 private deps: { Log: unknown };\n\
             \x20 constructor(deps: { Log: unknown }) { this.deps = deps; }\n\
             }\n",
        );
        assert!(js.contains("class P"));
        assert!(
            js.contains("constructor(deps)"),
            "ctor param keeps name:\n{js}"
        );
        assert!(
            js.contains("this.deps = deps"),
            "assignment preserved:\n{js}"
        );
        assert!(!js.contains(": { Log"), "field/param types erased:\n{js}");
    }

    #[test]
    fn as_casts_and_unique_symbol_erased() {
        let js = strip(
            "export const Tok: unique symbol = Symbol(\"T\");\n\
             export const id = (v: string) => v as string;\n",
        );
        assert!(!js.contains("unique symbol"), "unique symbol erased:\n{js}");
        assert!(!js.contains(" as string"), "as-cast erased:\n{js}");
        assert!(js.contains("Symbol(\"T\")"));
    }

    #[test]
    fn invalid_source_is_an_error_not_a_panic() {
        let err = strip_types("const = = =;", "bad.ts");
        assert!(err.is_err(), "malformed source is an error");
    }
}
