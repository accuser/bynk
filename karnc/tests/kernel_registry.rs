//! v0.30.2 (ADR 0063): the enumerable kernel-method registry must not drift
//! from the checker's dispatch. Every method the registry lists is driven
//! through the real checker on a receiver of the right type; none may be
//! rejected as `method_not_found` (a phantom registry entry). Args are
//! omitted, so a recognised method surfaces only as an arity error — which is
//! fine; we assert solely on `method_not_found`.

use karnc::kernel_methods::{
    FLOAT_METHODS, INT_METHODS, KernelMethod, LIST_METHODS, MAP_METHODS, OPTION_METHODS,
    RESULT_METHODS, STRING_METHODS,
};

/// `(let binding, receiver expr, methods)` — a receiver of each kernel type.
fn cases() -> Vec<(&'static str, &'static [KernelMethod])> {
    vec![
        ("let i = 1", INT_METHODS),
        ("let fl = 1.0", FLOAT_METHODS),
        ("let st = \"x\"", STRING_METHODS),
        ("let li = [1]", LIST_METHODS),
        ("let op: Option[Int] = Some(1)", OPTION_METHODS),
        ("let re: Result[Int, String] = Ok(1)", RESULT_METHODS),
        ("let mp: Map[String, Int] = Map.empty()", MAP_METHODS),
    ]
}

fn binding_name(decl: &str) -> &str {
    // `let <name>...` — the identifier after `let`.
    decl["let ".len()..]
        .split(|c: char| c == ' ' || c == ':')
        .next()
        .unwrap()
}

#[test]
fn kernel_registry_pins_dispatch() {
    let cases = cases();
    let mut body = String::from("commons probe.registry\n  fn probe() -> Int {\n");
    for (decl, _) in &cases {
        body += &format!("    {decl}\n");
    }
    for (decl, methods) in &cases {
        let recv = binding_name(decl);
        for meth in *methods {
            // No args — a recognised method gives an arity error, never
            // `method_not_found`.
            body += &format!("    let _ = {recv}.{}()\n", meth.name);
        }
    }
    body += "    0\n  }\n";

    let diags = karnc::diagnose(&body);
    let phantom: Vec<_> = diags
        .iter()
        .filter(|d| d.error.category == "karn.types.method_not_found")
        .map(|d| d.error.message.clone())
        .collect();
    assert!(
        phantom.is_empty(),
        "registry lists method(s) the checker rejects as not-found:\n{}",
        phantom.join("\n")
    );
}

#[test]
fn registries_are_well_formed() {
    for methods in [
        INT_METHODS,
        FLOAT_METHODS,
        STRING_METHODS,
        LIST_METHODS,
        MAP_METHODS,
        OPTION_METHODS,
        RESULT_METHODS,
    ] {
        assert!(!methods.is_empty());
        for meth in methods {
            assert!(!meth.name.is_empty());
            // The signature begins with the method name (a `name(...) -> T`).
            assert!(
                meth.signature.starts_with(meth.name),
                "signature {:?} should lead with {:?}",
                meth.signature,
                meth.name
            );
        }
    }
}
