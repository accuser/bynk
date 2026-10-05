//! Guard: #1673 — the VS Code extension's `bynkServerVersion` pin names the last
//! *shipped* release, never a version that hasn't been released yet.
//!
//! The pin used to be rewritten to the workspace version on every increment by
//! `scripts/bump-version.sh`, so between releases it named a GitHub Release that
//! did not exist, and a freshly packaged VSIX could not download its server.
//! It is now moved only by `release.yml`'s `server-pin` job, after the release
//! is published. Whether that release exists needs the network, so ci.yml's
//! `extension` job checks it. This offline half runs everywhere, including the
//! stamp's "Validate the stamped tree" step: the pin is a plain `vX.Y.Z`, it is
//! never ahead of the workspace, and the bump script doesn't move it.

use std::path::PathBuf;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn read(path: &str) -> String {
    let path = repo().join(path);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"))
}

/// `"X.Y.Z"` as numbers, or `None` if it isn't exactly three numeric parts.
fn parse(v: &str) -> Option<(u64, u64, u64)> {
    let mut it = v.split('.').map(|p| p.parse::<u64>().ok());
    let parsed = (it.next()??, it.next()??, it.next()??);
    it.next().is_none().then_some(parsed)
}

/// The string value of the first `"key": "…"` line in `json`.
fn json_str<'a>(json: &'a str, key: &str) -> &'a str {
    let needle = format!("\"{key}\": \"");
    let start = json
        .find(&needle)
        .unwrap_or_else(|| panic!("no `{key}` in vscode-bynk/package.json"))
        + needle.len();
    &json[start..start + json[start..].find('"').unwrap()]
}

fn workspace_version() -> (u64, u64, u64) {
    let toml = read("Cargo.toml");
    let line = toml
        .lines()
        .find(|l| l.starts_with("version = "))
        .expect("workspace version");
    parse(line.trim_start_matches("version = ").trim_matches('"')).expect("X.Y.Z workspace version")
}

#[test]
fn the_server_pin_is_a_released_version_not_ahead_of_the_workspace() {
    let pkg = read("vscode-bynk/package.json");
    let pin = json_str(&pkg, "bynkServerVersion");
    let Some(v) = pin.strip_prefix('v').and_then(parse) else {
        panic!(
            "bynkServerVersion is `{pin}`; it must be a release tag `vX.Y.Z` (a \
             pre-release tag is never the pin)"
        );
    };
    let ws = workspace_version();
    assert!(
        v <= ws,
        "bynkServerVersion {pin} is ahead of the workspace version {}.{}.{}, so it \
         names a release that can't exist yet. The pin is the last shipped \
         release; release.yml's server-pin job moves it after a release.",
        ws.0,
        ws.1,
        ws.2
    );
}

/// The bump script must leave the pin alone: rewriting it to the workspace
/// version is exactly the coupling #1673 removed.
#[test]
fn the_bump_script_does_not_move_the_server_pin() {
    let script = read("scripts/bump-version.sh");
    let offending: Vec<&str> = script
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#'))
        .filter(|l| l.contains("bynkServerVersion") && l.contains("sed"))
        .collect();
    assert!(
        offending.is_empty(),
        "scripts/bump-version.sh rewrites bynkServerVersion again: {offending:?}. \
         The pin names the last shipped release and only release.yml moves it."
    );
}
