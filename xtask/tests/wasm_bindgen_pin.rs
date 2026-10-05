//! Drift guard: #1677 — every workflow that installs `wasm-bindgen-cli` pins
//! the exact version of the `wasm-bindgen` crate in `Cargo.lock`.
//!
//! The CLI generates the JS glue for the module the crate compiled, and the two
//! are a matched pair. The CLI's own check compares only a schema version, which
//! stays the same across many releases, so a mismatch can build cleanly. That
//! is how the pin sat at 0.2.126 for weeks after `Cargo.lock` moved to 0.2.127
//! (#1522), under a comment saying the two matched. This guard is copied from
//! `bynkc/tests/wrangler_prewarm.rs`: the lockfile is the truth, and a lockfile
//! bump that leaves a workflow behind fails here instead of going unnoticed.

use std::path::PathBuf;

const CLI: &str = "wasm-bindgen-cli@";

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// The `wasm-bindgen` crate's version in `Cargo.lock`. Exactly one entry, so a
/// lockfile carrying two (a duplicate dependency) fails loudly rather than
/// checking against whichever came first.
fn locked_version() -> String {
    let lock = std::fs::read_to_string(repo().join("Cargo.lock")).expect("read Cargo.lock");
    let versions: Vec<&str> = lock
        .split("[[package]]")
        .filter(|pkg| pkg.lines().any(|l| l.trim() == r#"name = "wasm-bindgen""#))
        .filter_map(|pkg| {
            pkg.lines()
                .find_map(|l| l.trim().strip_prefix("version = "))
                .map(|v| v.trim_matches('"'))
        })
        .collect();
    assert_eq!(
        versions.len(),
        1,
        "expected one `wasm-bindgen` in Cargo.lock, found {versions:?}"
    );
    versions[0].to_string()
}

/// Every `wasm-bindgen-cli@<version>` pin in `.github/workflows/`, as
/// `(workflow, version)`. Every workflow is scanned, so a new one that installs
/// the CLI is covered without being listed here.
fn pins() -> Vec<(String, String)> {
    let dir = repo().join(".github/workflows");
    let mut pins = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("read {dir:?}: {e}")) {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "yml" && e != "yaml") {
            continue;
        }
        let yaml = std::fs::read_to_string(&path).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        for line in yaml.lines().map(str::trim).filter(|l| !l.starts_with('#')) {
            if let Some(rest) = line.split(CLI).nth(1) {
                let version: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || *c == '.')
                    .collect();
                pins.push((name.clone(), version));
            }
        }
    }
    pins.sort();
    pins
}

#[test]
fn every_wasm_bindgen_cli_pin_matches_the_lockfile() {
    let locked = locked_version();
    let pins = pins();
    for (workflow, version) in &pins {
        assert_eq!(
            version, &locked,
            "{workflow} installs `{CLI}{version}`, but Cargo.lock has wasm-bindgen \
             {locked}. The CLI must be the same version as the crate: bump the pin \
             with the lockfile."
        );
    }
}

/// Non-vacuity: the two jobs that build the playground's wasm both pin the CLI.
/// If one stops installing it (or spells the install differently), the guard
/// above would pass over nothing for it.
#[test]
fn the_playground_builds_are_pinned() {
    let pins = pins();
    for workflow in ["ci.yml", "deploy-playground.yml"] {
        assert!(
            pins.iter().any(|(w, _)| w == workflow),
            "{workflow} no longer pins `{CLI}<version>`. If it stopped building \
             the playground's wasm, drop it here; if the install is spelled \
             differently now, teach `pins` the new spelling. Found: {pins:?}"
        );
    }
}
