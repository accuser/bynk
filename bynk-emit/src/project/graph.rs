use super::*;

/// #696: `sites` maps a consuming unit's name to the project-relative
/// `identity_path` and `consumes`-clause span of a representative clause. When a
/// cycle is detected the diagnostic is anchored on the closing unit's clause (a
/// real span in a real file) so the CLI renders ariadne source context; a unit
/// absent from `sites` (e.g. a synthetic adapter) yields an unattributed,
/// spanless diagnostic as before.
pub(crate) fn detect_consumes_cycles(
    consumes: &HashMap<String, Vec<String>>,
    sites: &HashMap<String, (PathBuf, Span)>,
    errors: &mut Vec<(Option<PathBuf>, CompileError)>,
) {
    // Tarjan / Kosaraju overkill — a simple DFS with a path stack catches
    // cycles and yields the cycle path for the diagnostic.
    let mut visited: HashSet<String> = HashSet::new();
    let mut reported: HashSet<Vec<String>> = HashSet::new();
    for start in consumes.keys() {
        if visited.contains(start) {
            continue;
        }
        let mut stack: Vec<String> = Vec::new();
        let mut on_stack: HashSet<String> = HashSet::new();
        dfs_consumes(
            start,
            consumes,
            sites,
            &mut visited,
            &mut stack,
            &mut on_stack,
            &mut reported,
            errors,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn dfs_consumes(
    node: &str,
    consumes: &HashMap<String, Vec<String>>,
    sites: &HashMap<String, (PathBuf, Span)>,
    visited: &mut HashSet<String>,
    stack: &mut Vec<String>,
    on_stack: &mut HashSet<String>,
    reported: &mut HashSet<Vec<String>>,
    errors: &mut Vec<(Option<PathBuf>, CompileError)>,
) {
    if on_stack.contains(node) {
        // Found a cycle: extract the path from `node`'s position in stack.
        let start = stack.iter().position(|s| s == node).unwrap_or(0);
        let mut cycle: Vec<String> = stack[start..].to_vec();
        cycle.push(node.to_string());
        // Canonicalise the cycle for de-dup.
        let canon = canonicalise_cycle(&cycle);
        if reported.insert(canon.clone()) {
            // Anchor the diagnostic on the closing unit's `consumes` clause when
            // its site is known (#696); fall back to the spanless, unattributed
            // form otherwise.
            let (file, span) = match sites.get(node) {
                Some((path, span)) => (Some(path.clone()), *span),
                None => (None, Span::default()),
            };
            errors.push((file, CompileError::new(
                "bynk.context.consumes_cycle",
                span,
                format!(
                    "`consumes` cycle detected: {}",
                    cycle.join(" → ")
                ),
            )
            .with_note(
                "units must form an acyclic `consumes` graph; remove one of the `consumes` clauses or restructure",
            )));
        }
        return;
    }
    if visited.contains(node) {
        return;
    }
    visited.insert(node.to_string());
    on_stack.insert(node.to_string());
    stack.push(node.to_string());
    if let Some(targets) = consumes.get(node) {
        for t in targets {
            dfs_consumes(
                t, consumes, sites, visited, stack, on_stack, reported, errors,
            );
        }
    }
    stack.pop();
    on_stack.remove(node);
}

fn canonicalise_cycle(cycle: &[String]) -> Vec<String> {
    if cycle.is_empty() {
        return Vec::new();
    }
    // Drop the duplicated last element (cycle vector ends with the start node).
    let body = &cycle[..cycle.len() - 1];
    if body.is_empty() {
        return Vec::new();
    }
    let mut min_idx = 0;
    for (i, s) in body.iter().enumerate() {
        if s < &body[min_idx] {
            min_idx = i;
        }
    }
    let mut rotated: Vec<String> = body[min_idx..].to_vec();
    rotated.extend(body[..min_idx].iter().cloned());
    rotated
}

/// v0.12: detect cycles in the provider dependency graph. Each provided
/// capability depends (via its provider's `given`) on other capabilities; a
/// cycle means the composition root cannot order instantiation. Emits
/// `bynk.provider.dependency_cycle` on every provider that participates in a
/// cycle. `providers` is keyed by capability name.
pub(crate) fn detect_provider_dependency_cycles(
    providers: &HashMap<String, ProviderDecl>,
    errors: &mut Vec<CompileError>,
) {
    fn visit(
        node: &str,
        providers: &HashMap<String, ProviderDecl>,
        visited: &mut HashSet<String>,
        stack: &mut Vec<String>,
        in_stack: &mut HashSet<String>,
        cyclic: &mut HashSet<String>,
    ) {
        if visited.contains(node) {
            return;
        }
        in_stack.insert(node.to_string());
        stack.push(node.to_string());
        if let Some(p) = providers.get(node) {
            for dep in &p.given {
                // Cross-context dependencies follow the (acyclic) `consumes`
                // graph; only intra-context provider edges can form a cycle here.
                if dep.is_cross_context() {
                    continue;
                }
                // Only follow dependencies that have a provider in this context.
                if !providers.contains_key(dep.key()) {
                    continue;
                }
                if in_stack.contains(dep.key()) {
                    // A back-edge: everything from `dep` down the current stack
                    // is on the cycle.
                    let start = stack.iter().position(|n| n == dep.key()).unwrap_or(0);
                    for n in &stack[start..] {
                        cyclic.insert(n.clone());
                    }
                } else if !visited.contains(dep.key()) {
                    visit(dep.key(), providers, visited, stack, in_stack, cyclic);
                }
            }
        }
        stack.pop();
        in_stack.remove(node);
        visited.insert(node.to_string());
    }

    let mut visited: HashSet<String> = HashSet::new();
    let mut cyclic: HashSet<String> = HashSet::new();
    let mut keys: Vec<&String> = providers.keys().collect();
    keys.sort();
    for k in keys {
        let mut stack: Vec<String> = Vec::new();
        let mut in_stack: HashSet<String> = HashSet::new();
        visit(
            k,
            providers,
            &mut visited,
            &mut stack,
            &mut in_stack,
            &mut cyclic,
        );
    }

    let mut cyclic_sorted: Vec<&String> = cyclic.iter().collect();
    cyclic_sorted.sort();
    for cap in cyclic_sorted {
        if let Some(p) = providers.get(cap) {
            errors.push(
                CompileError::new(
                    "bynk.provider.dependency_cycle",
                    p.span,
                    format!(
                        "provider `{}` for capability `{}` is part of a capability dependency cycle",
                        p.provider_name.name, cap,
                    ),
                )
                .with_note(
                    "a capability cannot depend on itself, directly or transitively, through \
                     provider `given`",
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn strs(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|x| (*x).to_string()).collect()
    }

    fn graph(edges: &[(&str, &[&str])]) -> HashMap<String, Vec<String>> {
        edges
            .iter()
            .map(|(k, vs)| ((*k).to_string(), strs(vs)))
            .collect()
    }

    // -- canonicalise_cycle (the dedup key for consumes cycles) ---------------
    #[test]
    fn canonicalise_cycle_is_stable_across_rotations() {
        // Input always ends with the duplicated start node (the dfs_consumes
        // shape); every rotation of one cycle yields the same canonical key.
        assert_eq!(
            canonicalise_cycle(&strs(&["a", "b", "c", "a"])),
            strs(&["a", "b", "c"])
        );
        assert_eq!(
            canonicalise_cycle(&strs(&["b", "c", "a", "b"])),
            strs(&["a", "b", "c"])
        );
        assert_eq!(
            canonicalise_cycle(&strs(&["c", "a", "b", "c"])),
            strs(&["a", "b", "c"])
        );
    }

    #[test]
    fn canonicalise_cycle_edge_cases() {
        assert_eq!(canonicalise_cycle(&[]), Vec::<String>::new());
        assert_eq!(canonicalise_cycle(&strs(&["a", "a"])), strs(&["a"])); // self-loop
        assert_eq!(
            canonicalise_cycle(&strs(&["b", "a", "b"])),
            strs(&["a", "b"])
        );
    }

    // -- detect_consumes_cycles over synthetic adjacency maps -----------------
    #[test]
    fn detect_consumes_cycles_silent_on_acyclic() {
        let g = graph(&[("a", &["b"]), ("b", &["c"]), ("c", &[])]);
        let mut errors = Vec::new();
        detect_consumes_cycles(&g, &no_sites(), &mut errors);
        assert!(errors.is_empty());
    }

    #[test]
    fn detect_consumes_cycles_reports_each_cycle_once() {
        let mut e2 = Vec::new();
        detect_consumes_cycles(
            &graph(&[("a", &["b"]), ("b", &["a"])]),
            &no_sites(),
            &mut e2,
        );
        assert_eq!(e2.len(), 1);

        let mut e3 = Vec::new();
        detect_consumes_cycles(
            &graph(&[("a", &["b"]), ("b", &["c"]), ("c", &["a"])]),
            &no_sites(),
            &mut e3,
        );
        assert_eq!(e3.len(), 1);

        let mut eself = Vec::new();
        detect_consumes_cycles(&graph(&[("a", &["a"])]), &no_sites(), &mut eself);
        assert_eq!(eself.len(), 1);
    }

    #[test]
    fn detect_consumes_cycles_reports_disjoint_cycles_separately() {
        let mut errors = Vec::new();
        detect_consumes_cycles(
            &graph(&[("a", &["b"]), ("b", &["a"]), ("c", &["d"]), ("d", &["c"])]),
            &no_sites(),
            &mut errors,
        );
        assert_eq!(errors.len(), 2);
    }

    // #696: with a known consumes-site the cycle diagnostic is anchored on the
    // closing unit's clause (real span + owning file) so the CLI can render it
    // with ariadne source context; without one it stays spanless/unattributed.
    #[test]
    fn detect_consumes_cycles_attributes_to_closing_units_site() {
        // Self-loops keep the closing node deterministic (a multi-unit cycle
        // closes on whichever unit the key-ordered DFS reaches first).
        let sites: HashMap<String, (PathBuf, Span)> =
            [("a".to_string(), (PathBuf::from("a.bynk"), Span::new(3, 8)))]
                .into_iter()
                .collect();
        let mut errors = Vec::new();
        detect_consumes_cycles(&graph(&[("a", &["a"])]), &sites, &mut errors);
        assert_eq!(errors.len(), 1);
        let (file, err) = &errors[0];
        assert_eq!(file.as_deref(), Some(Path::new("a.bynk")));
        assert_eq!(err.span, Span::new(3, 8));

        // A cycle whose closing unit has no recorded site stays unattributed.
        let mut bare = Vec::new();
        detect_consumes_cycles(&graph(&[("x", &["x"])]), &sites, &mut bare);
        assert_eq!(bare.len(), 1);
        assert_eq!(bare[0].0, None);
        assert_eq!(bare[0].1.span, Span::default());
    }

    fn no_sites() -> HashMap<String, (PathBuf, Span)> {
        HashMap::new()
    }
}
