use super::*;

/// The project's binding graph: worker → the workers it binds to.
pub(crate) fn service_graph(
    resources: &BTreeMap<String, Resources>,
) -> BTreeMap<String, Vec<String>> {
    resources
        .iter()
        .map(|(worker, r)| (worker.clone(), r.binds_to.clone()))
        .collect()
}

/// Why an order could not be produced.
#[derive(Debug, PartialEq, Eq)]
pub enum OrderError {
    /// A `consumes` cycle. Unreachable through the compiler — `bynkc` rejects
    /// one as `bynk.context.consumes_cycle` before emit, and `deploy` compiles
    /// first — so this is defence in depth against a hand-edited build tree,
    /// not a user-facing path. Named rather than silently tolerated because the
    /// alternative under Cloudflare's upload-time resolution is a deploy that
    /// cannot be completed in one pass.
    Cycle(Vec<String>),
}

impl std::fmt::Display for OrderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OrderError::Cycle(path) => write!(
                f,
                "the generated Service Bindings form a cycle ({}) — it cannot be uploaded in dependency order",
                path.join(" → ")
            ),
        }
    }
}

/// Order the workers so that **every binding target is uploaded before the
/// worker that binds to it** — dependencies first.
///
/// This is a correctness barrier, not a nicety: Cloudflare resolves a Service
/// Binding at **upload time**, and rejects a Worker whose bound target does not
/// yet exist ("deployment will fail, because Worker A declares a binding to
/// Worker B, which does not yet exist"). Uploading in a wrong order does not
/// merely open a transient half-wired window — it fails outright.
///
/// Depth-first post-order over the sorted worker list, so the result is
/// deterministic for a given project rather than dependent on map iteration
/// order. Pure, so the ordering contract is unit-tested without a build tree.
pub fn deploy_order(
    workers: &[String],
    graph: &BTreeMap<String, Vec<String>>,
) -> Result<Vec<String>, OrderError> {
    let mut order = Vec::new();
    let mut done = BTreeSet::new();
    let mut path = Vec::new();
    for worker in workers {
        visit(worker, graph, &mut done, &mut path, &mut order)?;
    }
    Ok(order)
}

fn visit(
    worker: &str,
    graph: &BTreeMap<String, Vec<String>>,
    done: &mut BTreeSet<String>,
    path: &mut Vec<String>,
    order: &mut Vec<String>,
) -> Result<(), OrderError> {
    if done.contains(worker) {
        return Ok(());
    }
    if path.iter().any(|p| p == worker) {
        // Close the reported path onto the repeated node so the cycle reads as
        // a loop (`a → b → a`) rather than a bare list.
        let start = path.iter().position(|p| p == worker).unwrap_or(0);
        let mut cycle: Vec<String> = path[start..].to_vec();
        cycle.push(worker.to_string());
        return Err(OrderError::Cycle(cycle));
    }
    path.push(worker.to_string());
    for target in graph.get(worker).into_iter().flatten() {
        // A binding to something outside this project's build (an
        // externally-managed Worker) has no node to order against; leave it to
        // Cloudflare to accept or reject.
        if graph.contains_key(target) {
            visit(target, graph, done, path, order)?;
        }
    }
    path.pop();
    done.insert(worker.to_string());
    order.push(worker.to_string());
    Ok(())
}

/// The contexts `worker` binds to that the ledger has never recorded as
/// deployed — the D4 gate for `--context`. Deploying a Worker whose binding
/// target does not exist fails at upload, so naming the absent dependency beats
/// letting Cloudflare reject the push with its own vocabulary.
pub(crate) fn absent_dependencies(
    worker: &str,
    graph: &BTreeMap<String, Vec<String>>,
    lock: &DeployLock,
    environment: &str,
) -> Vec<String> {
    graph
        .get(worker)
        .into_iter()
        .flatten()
        .filter(|target| !lock.is_deployed(environment, target))
        .cloned()
        .collect()
}

/// One dependency service whose live contract is not the one `worker` was
/// compiled against.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ContractSkew {
    pub(crate) dependency: String,
    pub(crate) service: String,
    pub(crate) expected: String,
    pub(crate) live: String,
}

/// v0.177 (#643): the D4 gate extended from *exists* to *matches*.
///
/// `deploy --context A` pushes A alone, against dependencies assumed already
/// live. D4 already refuses when a dependency has never been deployed. This adds
/// the other half: a dependency that exists but no longer provides the contract A
/// was compiled against. Without it, the push succeeds and the skew is
/// discovered by production traffic 409ing — which is better than the silent
/// misinterpretation that preceded this increment, but far worse than refusing.
///
/// **Silence is not a match.** A dependency the ledger has no contract record
/// for (deployed by a pre-v0.177 driver) yields no finding: the gate reports what
/// it *knows* is skewed, never what it merely cannot rule out. The runtime check
/// remains the backstop for exactly that case — and for a `wrangler` push behind
/// the driver's back, or a ledger that has drifted. This gate is an optimisation
/// over the runtime check, never a replacement for it.
pub(crate) fn contract_skews(
    expects: &BTreeMap<String, BTreeMap<String, String>>,
    lock: &DeployLock,
    worker_of: impl Fn(&str) -> String,
    environment: &str,
) -> Vec<ContractSkew> {
    let mut out = Vec::new();
    for (dependency, services) in expects {
        let worker = worker_of(dependency);
        // No record — a pre-v0.177 push, or never deployed. Silence is not a
        // match: report only what is *known* to be skewed. (An empty-but-present
        // record is different, and is checked: it means the callee is known to
        // provide nothing, so every expected service is absent.)
        let Some(live) = lock.live_contracts(environment, &worker) else {
            continue;
        };
        for (service, expected) in services {
            match live.get(service) {
                Some(actual) if actual != expected => out.push(ContractSkew {
                    dependency: dependency.clone(),
                    service: service.clone(),
                    expected: expected.clone(),
                    live: actual.clone(),
                }),
                // A service the live callee no longer provides at all: it was
                // removed or renamed, which is the most complete skew there is.
                None => out.push(ContractSkew {
                    dependency: dependency.clone(),
                    service: service.clone(),
                    expected: expected.clone(),
                    live: "<absent>".to_string(),
                }),
                Some(_) => {}
            }
        }
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::deploy::config::tests::project;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// A binding graph literal: `[("a", &["b"])]` = a binds to b.
    pub(crate) fn graph(edges: &[(&str, &[&str])]) -> BTreeMap<String, Vec<String>> {
        edges
            .iter()
            .map(|(from, to)| (from.to_string(), names(to)))
            .collect()
    }

    // ---- #601 slice 2: the deploy order --------------------------------

    #[test]
    fn a_binding_target_is_deployed_before_the_worker_that_binds_to_it() {
        // The defining contract. Cloudflare resolves a Service Binding at
        // upload and rejects a Worker whose target does not exist yet, so
        // this order is a correctness barrier, not a nicety.
        let g = graph(&[
            ("commerce-orders", &["commerce-payment"]),
            ("commerce-payment", &[]),
        ]);
        assert_eq!(
            deploy_order(&names(&["commerce-orders", "commerce-payment"]), &g),
            Ok(names(&["commerce-payment", "commerce-orders"])),
            "payment must be uploaded before the orders worker that binds to it"
        );
    }

    #[test]
    fn a_chain_deploys_from_the_far_end() {
        // a → b → c: c has no dependencies, so it goes first.
        let g = graph(&[("a", &["b"]), ("b", &["c"]), ("c", &[])]);
        assert_eq!(
            deploy_order(&names(&["a", "b", "c"]), &g),
            Ok(names(&["c", "b", "a"]))
        );
    }

    #[test]
    fn a_diamond_deploys_the_shared_dependency_once_and_first() {
        //   a → b → d
        //   a → c → d
        let g = graph(&[("a", &["b", "c"]), ("b", &["d"]), ("c", &["d"]), ("d", &[])]);
        let order = deploy_order(&names(&["a", "b", "c", "d"]), &g).expect("a diamond is acyclic");
        assert_eq!(
            order.len(),
            4,
            "the shared dependency is deployed once: {order:?}"
        );
        let at = |w: &str| order.iter().position(|o| o == w).expect("present");
        assert!(at("d") < at("b") && at("d") < at("c"), "{order:?}");
        assert!(at("b") < at("a") && at("c") < at("a"), "{order:?}");
    }

    #[test]
    fn the_order_is_deterministic_for_a_given_project() {
        // Two independent workers keep the discovered (sorted) order, so a
        // plan a reviewer approved is the plan that runs.
        let g = graph(&[("a", &[]), ("b", &[])]);
        assert_eq!(
            deploy_order(&names(&["a", "b"]), &g),
            Ok(names(&["a", "b"]))
        );
    }

    #[test]
    fn a_binding_outside_the_project_is_left_to_cloudflare() {
        // An externally-managed Worker has no node to order against; it must
        // not be invented into the order.
        let g = graph(&[("a", &["someone-elses-worker"])]);
        assert_eq!(deploy_order(&names(&["a"]), &g), Ok(names(&["a"])));
    }

    #[test]
    fn a_cycle_is_reported_rather_than_looping_forever() {
        // Defence in depth: `bynkc` rejects a `consumes` cycle
        // (`bynk.context.consumes_cycle`) before emit and `deploy` compiles
        // first, so this is unreachable through the compiler — but a
        // hand-edited build tree must not hang or overflow the stack.
        let g = graph(&[("a", &["b"]), ("b", &["a"])]);
        let Err(OrderError::Cycle(path)) = deploy_order(&names(&["a", "b"]), &g) else {
            panic!("a cycle must be reported");
        };
        assert_eq!(
            path.first(),
            path.last(),
            "the path closes into a loop: {path:?}"
        );
        assert!(path.contains(&"a".to_string()) && path.contains(&"b".to_string()));
    }

    #[test]
    fn a_self_binding_is_reported_not_looped() {
        let g = graph(&[("a", &["a"])]);
        assert!(matches!(
            deploy_order(&names(&["a"]), &g),
            Err(OrderError::Cycle(_))
        ));
    }

    #[test]
    fn the_graph_is_the_binding_edges_of_the_resources_read() {
        let resources = project(vec![
            (
                "orders",
                Resources::default().binds(&["payment"]).consumes(&["q"]),
            ),
            ("payment", Resources::default().needs_kv()),
        ]);
        assert_eq!(
            service_graph(&resources),
            graph(&[("orders", &["payment"]), ("payment", &[])]),
            "the graph carries the binding edges and nothing else"
        );
    }
}
