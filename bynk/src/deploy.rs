//! `bynk deploy` — provision persistent Cloudflare identity, then publish.
//!
//! The generated `wrangler.toml` is deliberately disposable. This module owns
//! the small, committed `bynk.deploy.lock` ledger and materialises its KV ids
//! into a freshly compiled worker immediately before Wrangler sees it.
//!
//! The command mints real Cloudflare resources and writes secrets, so it is
//! split by concern rather than kept as one file — following the layout
//! `bynk-emit/src/project.rs` established: this parent carries the shared
//! imports, the `mod` declarations, and the re-exports external callers see,
//! while each child opens with `use super::*;` and documents its own items.
//!
//! - `config.rs` — the generated `wrangler.toml` / build-output model, and the
//!   `[env.<name>]` synthesis a non-default `--env` needs.
//! - `graph.rs` — the binding graph, the upload order it forces, and the
//!   deploy-time contract-skew check.
//! - `ledger.rs` — the committed `bynk.deploy.lock`: what this project has
//!   provisioned, the orphan diff against it, and `--prune`.
//! - `pins.rs` — a declared foreign caller's pin (`bynk.foreign.toml`) and the
//!   gate that refuses a build breaking it (I4, #1970).
//! - `provisioning.rs` — every call out to the `wrangler` CLI.
//! - `secrets.rs` — which secrets a run sets, and where each value comes from.
//! - `plan.rs` — the plan/apply flow that drives all of the above.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::process::{ExitCode, Stdio};

use serde::{Deserialize, Serialize};

use crate::compiler::Compiler;
use crate::doctor::{self, Capability, Context, DoctorOptions, Report};
use crate::probe::{self, DetectOpts, Provenance, Toolbox};
use crate::report::{self, Format};
use crate::shell::exit_status_byte;
use crate::workers;

const LOCK_FILE: &str = "bynk.deploy.lock";
// P7.4 (#1305): every non-test use of the placeholder now goes through
// `bynk_emit::emitter::wrangler`'s own structural functions
// (`materialise_kv_namespace_ids`/`kv_bindings_needing_ids`) —
// this name survives only for `deploy/ledger.rs`'s own test fixtures, which
// build a minimal `wrangler.toml` containing it.
#[cfg(test)]
use bynk_emit::emitter::wrangler::KV_NAMESPACE_ID_PLACEHOLDER;

mod config;
mod graph;
mod ledger;
mod pins;
mod plan;
mod provisioning;
mod secrets;

use config::*;
use graph::*;
use ledger::*;
use pins::*;
use plan::*;
use provisioning::*;
use secrets::*;

// External facade: the paths `main.rs` and `dev.rs` already use must keep
// resolving exactly as they did before the split.
pub use ledger::materialise_deploy_state;
pub(crate) use plan::conflicting_env_passthrough;
pub use plan::{DeployFormat, DeployOptions, run};

/// I2 (#1971): for `bynk dev`, the names of the edge keys each built Worker
/// holds, and (I3) its foreign callers' secrets (a Worker with none is absent), read from the build's own
/// configurations as `bynk deploy` reads them. An edge between contexts whose
/// names no key can be derived from is an error, as it is at deploy.
pub(crate) fn local_edge_keys(
    workers_dir: &Path,
    workers: &[String],
) -> Result<BTreeMap<String, Vec<String>>, String> {
    let resources = project_resources(workers_dir, workers)?;
    let edges = project_edges(&resources);
    for edge in &edges {
        edge.ends()?;
    }
    // I3 (#1971): and the secret of each foreign caller a Worker admits, so a
    // local foreign caller signing under the local key is admitted too.
    Ok(workers
        .iter()
        .map(|w| {
            let mut keys = edge_keys_of(w, &edges);
            keys.extend(
                resources[w]
                    .foreign_callers
                    .iter()
                    .map(|f| f.binding.clone()),
            );
            (w.clone(), keys)
        })
        .filter(|(_, keys)| !keys.is_empty())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use config::tests::project;
    use ledger::tests::{lock_with_deployed, with_kv, with_kv_instance, with_queue};
    use plan::tests::plan_of;
    use secrets::tests::source;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// The guide's worked example: `commerce-orders` binds to
    /// `commerce-payment`, which is the one with the KV namespace.
    fn chain() -> BTreeMap<String, Resources> {
        project(vec![
            (
                "commerce-orders",
                Resources::default().binds(&["commerce-payment"]),
            ),
            ("commerce-payment", Resources::default().needs_kv()),
        ])
    }

    /// The goldens live beside the integration ones (`tests/golden/`) and bless
    /// identically — `BYNK_BLESS=1 cargo test -p bynk`. They are driven from
    /// here rather than from `tests/` because `derive_plan` reads the ledger and
    /// the binding graph, which are this module's private types: goldening the
    /// output must not force them into the crate's public API.
    fn bless_or_assert(name: &str, actual: &str) {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden")
            .join(name);
        if std::env::var_os("BYNK_BLESS").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, actual).unwrap();
            return;
        }
        let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
            panic!(
                "missing golden {}; regenerate with BYNK_BLESS=1 cargo test -p bynk",
                path.display()
            )
        });
        assert_eq!(
            actual, expected,
            "golden {name} drifted; re-bless with BYNK_BLESS=1 cargo test -p bynk"
        );
    }

    /// #601/#600: the plan is what `--dry-run` shows and the deploy guide
    /// quotes, so it is pinned exactly — the `order` line (slice 2's
    /// load-bearing claim), the queue and Durable Object lines (slice 1's, the
    /// latter reshaped by #1796), and the
    /// JSON shape, which is a documented machine-readable surface.
    #[test]
    fn golden_deploy_plan() {
        let chain_order = names(&["commerce-payment", "commerce-orders"]);

        let mut out = String::new();

        // Slice 0's shape: one context, nothing recorded. No `order` line —
        // there is no ordering claim to make about a single worker.
        out.push_str("# one context, first deploy\n");
        out.push_str(&plan_report(
            &plan_of(
                &names(&["api"]),
                &project(vec![("api", Resources::default().needs_kv())]),
                &DeployLock::default(),
            ),
            DeployFormat::Short,
        ));

        // The guide's worked example: payment first, because orders binds to it.
        out.push_str("\n# several contexts, first deploy\n");
        out.push_str(&plan_report(
            &plan_of(&chain_order, &chain(), &DeployLock::default()),
            DeployFormat::Short,
        ));

        // I3 (#1971): a context admitting two foreign callers lists each
        // one's secret as managed, beside its edge keys.
        out.push_str("\n# I3: foreign callers' secrets\n");
        let mut foreign = chain();
        foreign
            .get_mut("commerce-payment")
            .expect("the chain's callee")
            .foreign_callers = vec![
            ForeignCaller {
                actor: "Ledger".into(),
                binding: "LEDGER_CALL_KEY".into(),
                worker: Some("ledger-svc".into()),
            },
            ForeignCaller {
                actor: "Legacy".into(),
                binding: "LEGACY_CALL_KEY".into(),
                worker: None,
            },
        ];
        out.push_str(&plan_report(
            &plan_of(&chain_order, &foreign, &DeployLock::default()),
            DeployFormat::Short,
        ));

        // A re-run re-pushes rather than skipping, so the word is `redeploy`
        // and the namespace is reused. The ledger records the KV *before* the
        // push (ADR 0180), so a deployed context always has its namespace
        // recorded too — depict that state, not an unreachable one.
        out.push_str("\n# several contexts, already live — a re-run re-pushes\n");
        out.push_str(&plan_report(
            &plan_of(
                &chain_order,
                &chain(),
                &with_kv(
                    lock_with_deployed(&["commerce-payment", "commerce-orders"]),
                    "commerce-payment",
                ),
            ),
            DeployFormat::Short,
        ));

        // Slice 1's kinds. The Durable Object line is advisory in both states, so it
        // reads the same before and after — that sameness is the point, and the
        // golden is where it is visible.
        out.push_str("\n# slice 1: an agent and a queue, first deploy\n");
        out.push_str(&plan_report(
            &plan_of(
                &names(&["jobs"]),
                &project(vec![(
                    "jobs",
                    Resources::default()
                        .needs_kv()
                        .consumes(&["job-intake"])
                        .exports(&["JobLedger"]),
                )]),
                &DeployLock::default(),
            ),
            DeployFormat::Short,
        ));

        out.push_str("\n# slice 1: the same context, already provisioned\n");
        out.push_str(&plan_report(
            &plan_of(
                &names(&["jobs"]),
                &project(vec![(
                    "jobs",
                    Resources::default()
                        .needs_kv()
                        .consumes(&["job-intake"])
                        .exports(&["JobLedger"]),
                )]),
                &with_queue(with_kv(lock_with_deployed(&["jobs"]), "jobs"), "job-intake"),
            ),
            DeployFormat::Short,
        ));

        // Slice 3. The origin mark is the load-bearing part: `declared` is the
        // compiler's word, `supplied` is the user's, and a reader must not take
        // the absence of a `declared` line for "this context needs no secret".
        out.push_str("\n# slice 3: a declared auth secret, and one the user supplied\n");
        out.push_str(&plan_report(
            &derive_plan(
                &names(&["api"]),
                &project(vec![(
                    "api",
                    Resources::default().declares(&["AUTH_JWT_SECRET"]),
                )]),
                &DeployLock::default(),
                &source(&[("STRIPE_KEY", "sk_live_x")], &[]),
                false,
                "default",
            ),
            DeployFormat::Short,
        ));

        // `--force`: the action is `overwrite` rather than `set`. Presence is
        // absent from the plan by design — it is a live question, and the plan
        // is derived before auth so `--dry-run` stays offline.
        out.push_str("\n# slice 3: --force overwrites rather than setting if absent\n");
        out.push_str(&plan_report(
            &derive_plan(
                &names(&["api"]),
                &project(vec![(
                    "api",
                    Resources::default().declares(&["AUTH_JWT_SECRET"]),
                )]),
                &lock_with_deployed(&["api"]),
                &source(&[], &["PROBE_TOKEN"]),
                true,
                "default",
            ),
            DeployFormat::Short,
        ));

        // A supplied name goes to *every* context in the run: nothing says which
        // contexts read a `bynk.Secrets` name. The plan lists it per context so
        // that spread is visible rather than implied.
        out.push_str("\n# slice 3: a supplied secret reaches every context\n");
        out.push_str(&plan_report(
            &derive_plan(
                &chain_order,
                &chain(),
                &DeployLock::default(),
                &source(&[("SHARED_KEY", "v")], &[]),
                false,
                "default",
            ),
            DeployFormat::Short,
        ));

        // The three classes side by side — the increment's whole surface. A
        // reader must be able to tell the compiler's *required* knowledge
        // (`declared`) from its *advisory* knowledge (`read`) from the user's
        // word (`supplied`), because they fail differently.
        out.push_str("\n# all three classes: declared (required), read (advisory), supplied\n");
        out.push_str(&plan_report(
            &derive_plan(
                &names(&["api"]),
                &project(vec![(
                    "api",
                    Resources::default()
                        .declares(&["AUTH_JWT_SECRET"])
                        .reads(&["STRIPE_KEY"]),
                )]),
                &DeployLock::default(),
                &source(&[], &["PROBE_TOKEN"]),
                false,
                "default",
            ),
            DeployFormat::Short,
        ));

        out.push_str("\n# --format json\n");
        out.push_str(&plan_report(
            &plan_of(&chain_order, &chain(), &DeployLock::default()),
            DeployFormat::Json,
        ));

        // A computed name: the list is not a census, and the JSON is where a CI
        // job learns that rather than trusting a short list.
        out.push_str("\n# --format json, a context that computes a secret name\n");
        out.push_str(&plan_report(
            &derive_plan(
                &names(&["api"]),
                &project(vec![(
                    "api",
                    Resources::default()
                        .reads(&["WELL_KNOWN"])
                        .reads_incompletely(),
                )]),
                &DeployLock::default(),
                &SecretSource::default(),
                false,
                "default",
            ),
            DeployFormat::Json,
        ));

        // The JSON shape of slice 3's kinds — the surface a CI job reads to
        // learn which names it must supply, and which the compiler already knows.
        out.push_str("\n# --format json, with declared and supplied secrets\n");
        out.push_str(&plan_report(
            &derive_plan(
                &names(&["api"]),
                &project(vec![(
                    "api",
                    Resources::default().declares(&["AUTH_JWT_SECRET", "WH_SECRET"]),
                )]),
                &DeployLock::default(),
                &source(&[("STRIPE_KEY", "sk_live_x")], &["PROBE_TOKEN"]),
                false,
                "default",
            ),
            DeployFormat::Json,
        ));

        // The JSON shape of slice 1's kinds — the surface a CI job reads to
        // learn that the Durable Object namespaces are not ours to claim.
        out.push_str("\n# --format json, with a queue and a durable object\n");
        out.push_str(&plan_report(
            &plan_of(
                &names(&["jobs"]),
                &project(vec![(
                    "jobs",
                    Resources::default()
                        .consumes(&["job-intake"])
                        .exports(&["JobLedger"]),
                )]),
                &DeployLock::default(),
            ),
            DeployFormat::Json,
        ));

        // S4b (#1967): derived KV instances. Each is its own namespace, titled
        // `<worker>-<capability>`; the bare `KV` keeps its line as it was.
        let cart = || {
            project(vec![(
                "shop-cart",
                Resources::default()
                    .needs_kv()
                    .kv_instances(&["KV_PRICE_CACHE", "KV_SESSIONS"]),
            )])
        };
        out.push_str("\n# S4b: the bare namespace and two derived instances, first deploy\n");
        out.push_str(&plan_report(
            &plan_of(&names(&["shop-cart"]), &cart(), &DeployLock::default()),
            DeployFormat::Short,
        ));

        // A renamed capability (`Carts` → `PriceCache`) is a new binding: a
        // create beside the reuse, and the old instance an orphan, never
        // pruned without `--prune`'s confirmation. Under `--env staging` the
        // instances' titles are qualified; the bare one's is not.
        out.push_str("\n# S4b: an instance renamed, under --env staging\n");
        let mut staged = DeployLock::default();
        staged.record_deployed("staging", "shop-cart", Some(Default::default()));
        staged.record_kv("staging", "shop-cart", "KV", "bare-id");
        staged.record_kv("staging", "shop-cart", "KV_SESSIONS", "sessions-id");
        staged.record_kv("staging", "shop-cart", "KV_CARTS", "carts-id");
        let cart_order = names(&["shop-cart"]);
        let cart_resources = cart();
        let renamed = derive_plan(
            &cart_order,
            &cart_resources,
            &staged,
            &SecretSource::default(),
            false,
            "staging",
        );
        out.push_str(&plan_report(&renamed, DeployFormat::Short));

        out.push_str("\n# --format json, the same rename\n");
        out.push_str(&plan_report(&renamed, DeployFormat::Json));

        out.push_str("\n# S4b: a removed context's bare namespace and instance, both orphans\n");
        out.push_str(&plan_report(
            &plan_of(
                &names(&["api"]),
                &project(vec![("api", Resources::default())]),
                &with_kv_instance(
                    with_kv(DeployLock::default(), "gone"),
                    "gone",
                    "KV_SESSIONS",
                ),
            ),
            DeployFormat::Short,
        ));

        // Interop I5 (#1972): Service Bindings to Workers outside the
        // project, bound and never provisioned or ordered against.
        out.push_str("\n# interop I5: a foreign Worker beside a project context\n");
        let foreign = project(vec![
            (
                "shop-checkout",
                Resources::default()
                    .binds(&["shop-payment"])
                    .foreign(&[("SERVICE_BILLING", "legacy-billing")]),
            ),
            ("shop-payment", Resources::default()),
        ]);
        let order = names(&["shop-payment", "shop-checkout"]);
        out.push_str(&plan_report(
            &plan_of(&order, &foreign, &DeployLock::default()),
            DeployFormat::Short,
        ));

        bless_or_assert("deploy-plan.txt", &out);
    }

    /// #601 D4: a failure stops the run and names what did not land. The count
    /// and the list must agree, and the context that just failed — already
    /// reported on its own line — must not be listed again here.
    #[test]
    fn golden_deploy_stopped() {
        let mut out = String::new();
        out.push_str("# the last context failed: nothing was left to withhold\n");
        out.push_str(&stopped_report(&[]));
        out.push_str("# one context was left\n");
        out.push_str(&stopped_report(&names(&["commerce-orders"])));
        out.push_str("# several were left\n");
        out.push_str(&stopped_report(&names(&[
            "commerce-orders",
            "commerce-shipping",
        ])));
        bless_or_assert("deploy-stopped.txt", &out);
    }

    #[test]
    fn the_stop_report_counts_only_what_is_left_and_agrees_with_its_list() {
        // The regression: the slice reported `order[i..]`, which included the
        // context that had just failed — so a 3-context run failing at the 2nd
        // said "1 more context was not deployed: b, c", naming two.
        assert_eq!(
            stopped_report(&[]),
            "",
            "the failure itself is already reported"
        );
        for n in 1..5usize {
            let rest = names(&["c0", "c1", "c2", "c3"][..n]);
            let report = stopped_report(&rest);
            let listed = report
                .split(" not deployed: ")
                .nth(1)
                .and_then(|tail| tail.split(". Re-run").next())
                .expect("the list sits between the count and the remedy");
            assert_eq!(
                listed.split(", ").count(),
                n,
                "the list names every withheld context: {report}"
            );
            let count = if n == 1 {
                "1 more context was".to_string()
            } else {
                format!("{n} further contexts were")
            };
            assert!(
                report.contains(&count),
                "the count states the number it lists: {report}"
            );
        }
    }
}
