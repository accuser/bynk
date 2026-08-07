//! P4.2 (#1122), [DECISION E]: pins the six diagnostics categories that go
//! silently missing from the LSP's project analysis the moment `bynk-ide`
//! repoints off `bynk_emit::project::analyse_project_with` onto
//! `bynk_check::analysis::analyse_project` — the entry point P4.1 (#1115)
//! built, which is diagnostically faithful to `run_checks`'s `Mode::Analyse`
//! arm **minus** the categories this file pins (see
//! `bynk_check::analysis`'s own module doc comment for the full accounting).
//!
//! Each fixture below is copied from a real `bynkc/tests/fixtures/negative`
//! case that the CLI (`bynk_emit::project::compile_project`) rejects with
//! the named category — so each one is a *known-real* violation, not a
//! guess at one. This crate cannot depend on `bynk-emit` at all (R10.2,
//! enforced on `bynk-ide` at the manifest level, and mirrored here by
//! convention — see `bynk-lsp/Cargo.toml`'s own dependency comment), so
//! unlike `bynk-check/tests/differential_analysis.rs`'s
//! `new_entry_point_omits_test_body_diagnostics` these fixtures cannot
//! re-run the pre-repoint path for a live "before" comparison; each pin is
//! instead a direct assertion that today's `bynk_ide::diagnose_project`
//! output lacks the category a real violation would carry — "pin the gap as
//! an assertion, not an absence", per the tracking issue's own framing.
//!
//! **Correction found while grounding this fixture set.** The tracking
//! issue's [DECISION E] counted `check_platform_lock` among six *live*
//! regressions. Reading `lock_violation` (`bynk-emit/src/project/
//! validate.rs`) shows its `Conflict` arm is "not yet reachable end-to-end
//! while only one platform ships native capabilities" (the function's own
//! doc comment) and its `Required` arm fires only when the native platform
//! disagrees with the *selected* one — but `analyse_project_with` (the
//! pre-repoint LSP path) always calls `run_checks` with `Platform::default()`
//! (Cloudflare) and `BuildTarget::Bundle` hardcoded (`project.rs:743-744`),
//! and `bynk.cloudflare` is the *only* platform-native unit that exists
//! (`bynk_check::firstparty::platform_of`). So `first != selected` can never
//! hold under the LSP's own hardcoded call, for any project, both before and
//! after this slice — `check_platform_lock` was already a gap in name only,
//! exactly like schema-registry reconciliation (category 1, which
//! `bynk_check::analysis`'s own doc comment already excludes for the
//! identical reason). `platform_lock_diagnostic_stays_absent` below pins
//! that non-regression directly, rather than a repoint-caused one.

use std::collections::HashMap;
use std::path::PathBuf;

/// A self-contained temp project (real files on disk — discovery walks the
/// filesystem). Returns a *complete* overlay for every file written, since
/// `bynk-project`'s discovery has no disk-read fallback (content-ownership
/// track, #1086, slice 5) — mirrors `bynk-check/tests/
/// differential_analysis.rs`'s `setup_project`.
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn setup_project(test_name: &str, files: &[(&str, &str)]) -> (Scratch, HashMap<PathBuf, String>) {
    let root = std::env::temp_dir().join(format!(
        "bynk-lsp-residual-gap-{test_name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create test root");
    let mut overlay = HashMap::new();
    for (rel, contents) in files {
        let p = root.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).expect("create parent");
        }
        std::fs::write(&p, contents).expect("write file");
        overlay.insert(p, (*contents).to_string());
    }
    (Scratch(root.clone()), overlay)
}

fn all_categories(pd: &bynk_ide::ProjectDiagnostics) -> Vec<&'static str> {
    pd.files
        .iter()
        .flat_map(|f| f.diagnostics.iter())
        .chain(pd.unattributed.iter())
        .map(|d| d.error.category)
        .collect()
}

/// Category 1 of 6: `check_messages_bundles` (`bynk.messages.missing_reference`
/// here — a bundle with no `@reference` block). Copied from
/// `bynkc/tests/fixtures/negative/480_messages_missing_reference`.
#[test]
fn messages_bundle_diagnostic_goes_missing() {
    const SRC: &str =
        "commons bundle\n\nuses bynk.locale\n\nmessages \"en\" {\n  \"a\" => \"b\"\n}\n";
    let (scratch, overlay) = setup_project("messages", &[("src/bundle.bynk", SRC)]);

    let pd = bynk_ide::diagnose_project(&scratch.0, &overlay);
    let categories = all_categories(&pd);
    assert!(
        !categories.contains(&"bynk.messages.missing_reference"),
        "this pins the accepted P4.2 regression — `check_messages_bundles` is not \
         called by `bynk_check::analysis::analyse_project`, so a bundle missing its \
         `@reference` block no longer gets a diagnostic through the LSP; once phase 5 \
         ports this check, tighten this assertion (and delete this comment) rather \
         than deleting the test: {categories:?}"
    );
}

/// Category 2 of 6: `check_locale_bundle_ambiguity`
/// (`bynk.locale.multiple_message_bundles`). Copied from
/// `bynkc/tests/fixtures/negative/496_locale_multiple_message_bundles`.
#[test]
fn locale_bundle_ambiguity_diagnostic_goes_missing() {
    const MSGS_A: &str = "commons msgs_a\n\nuses bynk.locale\n\nmessages \"en\" @reference {\n  \"greeting\" => \"Hello\"\n}\n";
    const MSGS_B: &str = "commons msgs_b\n\nuses bynk.locale\n\nmessages \"en\" @reference {\n  \"farewell\" => \"Bye\"\n}\n";
    const WEB: &str = "context web\n\nconsumes bynk { Locale }\nuses msgs_a\nuses msgs_b\n\nservice api {\n  on call() -> Effect[String] given Locale {\n    let tag <- Locale.current()\n    Effect.pure(tag)\n  }\n}\n";

    let (scratch, overlay) = setup_project(
        "locale_ambiguity",
        &[
            ("src/msgs_a.bynk", MSGS_A),
            ("src/msgs_b.bynk", MSGS_B),
            ("src/web.bynk", WEB),
        ],
    );

    let pd = bynk_ide::diagnose_project(&scratch.0, &overlay);
    let categories = all_categories(&pd);
    assert!(
        !categories.contains(&"bynk.locale.multiple_message_bundles"),
        "this pins the accepted P4.2 regression — `check_locale_bundle_ambiguity` is \
         not called by `bynk_check::analysis::analyse_project`, so a context reaching \
         two message-bundle commons while consuming `Locale` no longer gets a \
         diagnostic through the LSP: {categories:?}"
    );
}

/// Category 3 of 6: `check_event_subscriptions`
/// (`bynk.event.unknown_subscription` — a `from Events(...)` subscription
/// naming a misspelled event). Copied from `bynkc/tests/fixtures/negative/
/// 505_events_unknown_subscription` (target-independent: the check takes no
/// `BuildTarget`, so the source fixture's `target.txt = workers` is not
/// needed here — `analyse_project_with`'s hardcoded `BuildTarget::Bundle`
/// already ran this check unconditionally pre-repoint).
#[test]
fn event_subscription_diagnostic_goes_missing() {
    const ORDER: &str = "context commerce.order\n\nexports transparent { PaymentConfirmed }\nconsumes bynk { Events }\n\nevent PaymentConfirmed = {\n  orderId: String,\n}\n\nservice markPaid {\n  on call(orderId: String) -> Effect[()] given Events {\n    Events.emit[PaymentConfirmed](PaymentConfirmed { orderId: orderId })\n  }\n}\n";
    const NOTIFICATIONS: &str = "context commerce.notifications\n\nconsumes commerce.order\n\nservice OnPayment from Events(PaymentConfrimed) {\n  on event(e: PaymentConfrimed) -> Effect[()] {\n    ()\n  }\n}\n";

    let (scratch, overlay) = setup_project(
        "event_subscription",
        &[
            ("src/commerce/order.bynk", ORDER),
            ("src/commerce/notifications.bynk", NOTIFICATIONS),
        ],
    );

    let pd = bynk_ide::diagnose_project(&scratch.0, &overlay);
    let categories = all_categories(&pd);
    assert!(
        !categories.contains(&"bynk.event.unknown_subscription"),
        "this pins the accepted P4.2 regression — `check_event_subscriptions` is not \
         called by `bynk_check::analysis::analyse_project`, so a `from Events(...)` \
         subscription naming an undeclared event no longer gets a diagnostic through \
         the LSP: {categories:?}"
    );
}

/// Category 4 of 6 (a non-regression, see this file's module doc comment):
/// `check_platform_lock` (`bynk.target.vendor_required`). Fixture copied from
/// `bynkc/tests/fixtures/negative/150_kv_on_node_platform`, whose CLI-level
/// violation needs `--platform node` — `analyse_project_with` (pre-repoint)
/// always analysed as `Platform::default()` (Cloudflare)/`BuildTarget::Bundle`,
/// under which this exact fixture is *not* a violation (Cloudflare-native `Kv`
/// consumed while the selected platform is also Cloudflare). The category was
/// already absent from the LSP's diagnostics before this slice — this pins
/// that it stays absent, not that this slice took it away.
#[test]
fn platform_lock_diagnostic_stays_absent() {
    const STORE: &str = "context cache.store\n\nconsumes bynk.cloudflare { Kv }\n\nservice cache {\n  on call(key: String, value: String) -> Effect[Option[String]] given Kv {\n    let previous <- Kv.get(key)\n    let _ <- Kv.put(key, value)\n    let _ <- Kv.delete(\"stale\")\n    previous\n  }\n}\n";

    let (scratch, overlay) = setup_project("platform_lock", &[("src/cache/store.bynk", STORE)]);

    let pd = bynk_ide::diagnose_project(&scratch.0, &overlay);
    let categories = all_categories(&pd);
    assert!(
        !categories.contains(&"bynk.target.vendor_required"),
        "no platform-lock violation is reachable via `bynk_ide::diagnose_project` \
         either before or after P4.2 — the LSP always analyses as Cloudflare/Bundle, \
         under which this fixture is not a violation: {categories:?}"
    );
}

/// Category 5 of 6: `check_function_type_boundaries`
/// (`bynk.types.function_at_boundary` — a function type in a non-boundary
/// position, here a service handler parameter). Copied from `bynkc/tests/
/// fixtures/negative/152_fn_type_in_service_sig`.
#[test]
fn function_type_boundary_diagnostic_goes_missing() {
    const API: &str = "context hof.api\n\nservice runner {\n  on call(f: Int -> Int) -> Effect[Int] {\n    Effect.pure(0)\n  }\n}\n";

    let (scratch, overlay) = setup_project("fn_boundary", &[("src/hof/api.bynk", API)]);

    let pd = bynk_ide::diagnose_project(&scratch.0, &overlay);
    let categories = all_categories(&pd);
    assert!(
        !categories.contains(&"bynk.types.function_at_boundary"),
        "this pins the accepted P4.2 regression — `bynk_check::analysis::analyse_project` \
         calls `phase_group` with `None` for the function-type-boundary hook \
         (`check_function_type_boundaries` never runs), so a function type at a \
         service-handler boundary no longer gets a diagnostic through the LSP: \
         {categories:?}"
    );
}

/// Category 6 of 6: test/integration-suite processing — `process_tests` never
/// runs in the new entry point, so (a) a type error inside a `suite` case's
/// body reports no diagnostic, and (b) the suite body's own binding edges
/// (e.g. a call to a commons `fn`) are absent from `index` too, breaking
/// go-to-definition/find-references for a test-only binding. Same fixture as
/// `bynk-check/tests/differential_analysis.rs`'s
/// `new_entry_point_omits_test_body_diagnostics` (the analogous pin one layer
/// down, at the `bynk-check` entry point itself rather than through
/// `bynk-ide`'s own call path).
#[test]
fn test_body_diagnostics_and_index_bindings_go_missing() {
    const MATH_SRC: &str = "commons demo.math\n\nfn double(n: Int) -> Int { n * 2 }\n";
    const TEST_SRC: &str = "suite demo.math\n\ncase \"broken\" {\n  let x: Int = \"not an int\"\n  let y = double(1)\n  expect y == 2\n}\n";

    let (scratch, overlay) = setup_project(
        "test_body",
        &[
            ("demo/math.bynk", MATH_SRC),
            ("tests/math_test.bynk", TEST_SRC),
        ],
    );

    let pd = bynk_ide::diagnose_project(&scratch.0, &overlay);
    let categories = all_categories(&pd);
    assert!(
        !categories.contains(&"bynk.types.let_annotation_mismatch"),
        "this pins the accepted P4.2 regression — the test body's own type error no \
         longer gets a diagnostic through the LSP: {categories:?}"
    );

    let double_refs_outside_tests = pd
        .index
        .symbols
        .iter()
        .filter(|(k, _)| k.name == "double")
        .flat_map(|(_, entry)| entry.refs.iter())
        .filter(|site| site.path.components().any(|c| c.as_os_str() == "tests"))
        .count();
    assert_eq!(
        double_refs_outside_tests, 0,
        "the suite case's call to `double` must be absent from the index too — \
         `process_tests` never runs, so the reference is never recorded — this is \
         the concrete go-to-definition-inside-a-test-file regression"
    );
}
