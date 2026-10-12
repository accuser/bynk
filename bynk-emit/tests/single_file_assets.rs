//! #1883, slice 1a (review of #1903): the single-file path checks `asset`
//! declarations with the same rules as the project pipeline, and only a
//! build refuses them. `bynk check page.bynk` goes through
//! `check_with_warnings`; `bynkc compile page.bynk` through
//! `compile_with_warnings`.

const PAGE: &str = "commons compat.page\n\nasset style = \"assets/board.css\"\n\nfn sheet() -> String {\n  style.url\n}\n";

fn categories(errors: &[bynk_syntax::CompileError]) -> Vec<&'static str> {
    errors.iter().map(|e| e.category).collect()
}

#[test]
fn a_single_file_check_accepts_an_asset_and_a_build_refuses_it() {
    let warnings = bynk_emit::check_with_warnings(PAGE, "page.bynk")
        .unwrap_or_else(|e| panic!("check should pass, got {:?}", categories(&e)));
    assert!(warnings.is_empty(), "{:?}", categories(&warnings));

    let errors = match bynk_emit::compile_with_warnings(PAGE, "page.bynk") {
        Ok(_) => panic!("a build of an asset must be refused until slice 1b"),
        Err(e) => e,
    };
    assert_eq!(categories(&errors), vec!["bynk.asset.not_yet_emitted"]);
}

#[test]
fn a_single_file_check_applies_the_declaration_rules() {
    for (src, code) in [
        (
            "commons c\n\nasset style = \"a.css\"\nasset style = \"b.css\"\n",
            "bynk.asset.duplicate",
        ),
        (
            "commons c\n\nfn style() -> Int {\n  1\n}\n\nasset style = \"a.css\"\n",
            "bynk.resolve.name_conflict",
        ),
        (
            "commons c\n\ntype Style = Int\n\nasset Style = \"a.css\"\n",
            "bynk.resolve.name_conflict",
        ),
        (
            "commons c\n\nasset style = \"../x.css\"\n",
            "bynk.asset.unsafe_path",
        ),
    ] {
        let errors = match bynk_emit::check_with_warnings(src, "c.bynk") {
            Ok(_) => panic!("expected `{code}` for:\n{src}"),
            Err(e) => e,
        };
        assert!(
            categories(&errors).contains(&code),
            "expected `{code}`, got {:?} for:\n{src}",
            categories(&errors)
        );
        assert!(
            !categories(&errors).contains(&"bynk.asset.not_yet_emitted"),
            "a check must not report the build-only gate"
        );
    }
}
