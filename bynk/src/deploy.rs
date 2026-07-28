//! `bynk deploy` — provision persistent Cloudflare identity, then publish.
//!
//! The generated `wrangler.toml` is deliberately disposable. This module owns
//! the small, committed `bynk.deploy.lock` ledger and materialises its KV id
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
use bynk_emit::emitter::wrangler::KV_NAMESPACE_ID_PLACEHOLDER;

mod config;
mod graph;
mod ledger;
mod plan;
mod provisioning;
mod secrets;

use config::*;
use graph::*;
use ledger::*;
use plan::*;
use provisioning::*;
use secrets::*;

// External facade: the paths `main.rs` and `dev.rs` already use must keep
// resolving exactly as they did before the split.
pub use ledger::materialise_deploy_state;
pub(crate) use plan::conflicting_env_passthrough;
pub use plan::{DeployFormat, DeployOptions, run};

#[cfg(test)]
mod tests {
    use super::*;
    use config::tests::project;
    use ledger::tests::{lock_with_deployed, with_kv, with_queue};
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
    /// load-bearing claim), the queue and migration lines (slice 1's), and the
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

        // Slice 1's kinds. The migration line is advisory in both states, so it
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
                        .migrates("v1"),
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
                        .migrates("v1"),
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
        // learn that the migration is not ours to claim.
        out.push_str("\n# --format json, with a queue and a migration\n");
        out.push_str(&plan_report(
            &plan_of(
                &names(&["jobs"]),
                &project(vec![(
                    "jobs",
                    Resources::default()
                        .consumes(&["job-intake"])
                        .migrates("v1"),
                )]),
                &DeployLock::default(),
            ),
            DeployFormat::Json,
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
