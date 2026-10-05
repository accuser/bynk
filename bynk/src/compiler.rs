//! Locate the `bynkc` compiler the driver shells, and report
//! **driver↔compiler version skew**.
//!
//! Resolution order (ADR: introduce the `bynk` driver):
//!
//! 1. an explicit override — the `BYNK_BYNKC` environment variable (the
//!    `bynk.executablePath`-style escape hatch);
//! 2. `bynkc` on `PATH`;
//! 3. a `bynkc` sibling of the running `bynk` binary (mirrors how `vscode-bynk`
//!    resolves `bynkc-lsp` next to itself).
//!
//! An explicit override wins when set — an override that only applied after
//! auto-discovery failed would be useless. The skew check exists *because* this
//! resolution can pick a `bynkc` whose version differs from the driver's: once
//! they are separate binaries, a global `bynk 0.46` can shell a stale `bynkc
//! 0.44`, and `doctor`'s whole job is to surface exactly that.

use std::path::{Path, PathBuf};

use crate::probe::{Toolbox, Version};

/// How `bynkc` was located.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// From the `BYNK_BYNKC` override.
    Override,
    /// From the global `PATH`.
    Path,
    /// A sibling of the running `bynk` binary.
    Sibling,
}

impl Origin {
    pub fn token(self) -> &'static str {
        match self {
            Origin::Override => "override",
            Origin::Path => "path",
            Origin::Sibling => "sibling",
        }
    }
}

/// Driver↔compiler version relationship. Patch differences are ignored (they
/// are wire-compatible under the project's unified versioning); a minor drift
/// warns; a major drift is a contract mismatch and an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skew {
    /// Versions match (ignoring patch), or the compiler version is unknown.
    Match,
    /// Minor drift — warn (fails only under `--strict`).
    Minor,
    /// Major drift — a contract mismatch; an error even on a bare run.
    Major,
}

impl Skew {
    /// Classify the driver version against a resolved compiler version.
    pub fn classify(driver: Version, compiler: Version) -> Skew {
        if driver.major != compiler.major {
            Skew::Major
        } else if driver.minor != compiler.minor {
            Skew::Minor
        } else {
            Skew::Match
        }
    }

    pub fn token(self) -> &'static str {
        match self {
            Skew::Match => "match",
            Skew::Minor => "minor",
            Skew::Major => "major",
        }
    }
}

/// A resolved (or unresolved) `bynkc`.
#[derive(Debug, Clone)]
pub struct Compiler {
    /// `None` when `bynkc` could not be located at all — the broken compile
    /// floor, which fails `doctor` even on a bare run.
    pub path: Option<PathBuf>,
    pub origin: Option<Origin>,
    pub version: Option<Version>,
    /// `None` when there is no compiler, or its version could not be read.
    pub skew: Option<Skew>,
}

impl Compiler {
    pub fn is_resolved(&self) -> bool {
        self.path.is_some()
    }

    /// A major skew is a hard floor break even on a bare run.
    pub fn has_major_skew(&self) -> bool {
        self.skew == Some(Skew::Major)
    }
}

/// Resolve `bynkc` against a [`Toolbox`], given the override (typically
/// `std::env::var("BYNK_BYNKC")`), the directory of the running `bynk` binary
/// (for the sibling fallback), and the driver's own version (to classify skew).
pub fn resolve(
    tb: &dyn Toolbox,
    override_path: Option<&Path>,
    bynk_bin_dir: Option<&Path>,
    driver: Version,
) -> Compiler {
    let (path, origin) = locate(tb, override_path, bynk_bin_dir);
    let version = path.as_deref().and_then(|p| tb.version(p));
    let skew = version.map(|v| Skew::classify(driver, v));
    Compiler {
        path,
        origin,
        version,
        skew,
    }
}

fn locate(
    tb: &dyn Toolbox,
    override_path: Option<&Path>,
    bynk_bin_dir: Option<&Path>,
) -> (Option<PathBuf>, Option<Origin>) {
    // An empty override (`BYNK_BYNKC=""`) is treated as unset — resolving a
    // bare `bynkc` from the current directory was a mild path-hijack surface.
    if let Some(ovr) = override_path.filter(|p| !p.as_os_str().is_empty()) {
        // An explicit override is taken as-is when it resolves; we do not fall
        // through on a bad override, so a typo surfaces rather than silently
        // picking a different compiler. The lookup uses the override's full
        // file *name* (PATHEXT-aware on Windows), never its stem — a stem
        // lookup made `/dir/bynkc.backup` silently resolve `/dir/bynkc`, a
        // different binary than the one named.
        let dir = ovr
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let name = ovr.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if let Some(p) = tb.in_dir(dir, name) {
            return (Some(p), Some(Origin::Override));
        }
        // Set but not found: surface honestly. `doctor` renders its
        // "override set but not found" failure and delegation refuses with
        // the misconfigured path named — instead of reporting Ok and then
        // failing at spawn.
        return (None, Some(Origin::Override));
    }
    if let Some(p) = tb.on_path("bynkc") {
        return (Some(p), Some(Origin::Path));
    }
    if let Some(dir) = bynk_bin_dir
        && let Some(p) = tb.in_dir(dir, "bynkc")
    {
        return (Some(p), Some(Origin::Sibling));
    }
    (None, None)
}

/// #1675: the override for the skew gate below, beside `bynk test`'s
/// `--allow-skew` flag. Any non-empty value allows a skewed `bynkc` (the same
/// "non-empty means set" contract as the CI `BYNK_REQUIRE_*` switches).
pub const ALLOW_SKEW_ENV: &str = "BYNK_ALLOW_SKEW";

/// Whether the skew override is set in the environment.
pub fn skew_allowed_by_env() -> bool {
    std::env::var(ALLOW_SKEW_ENV).is_ok_and(|v| !v.is_empty())
}

/// #1675: what a command that is about to run a *second* compiler does about
/// its skew from the driver — the same classification `doctor` renders,
/// acted on. `bynk test` always shells `bynkc`, and `bynk dev`/`deploy` do
/// under a `BYNK_BYNKC` override, so on those paths a skewed `bynkc` could
/// check and test the code with a different compiler, many increments apart,
/// silently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkewVerdict {
    /// Matching (patch ignored), or the version is unknown: run, silently.
    Run,
    /// Run, after printing this warning.
    Warn(String),
    /// Do not run; print this error.
    Refuse(String),
}

/// Decide what `command` does about `compiler`'s skew. Minor skew warns;
/// major skew refuses unless `allow` (the `--allow-skew` flag or
/// `BYNK_ALLOW_SKEW`), in which case it warns instead.
pub fn skew_verdict(compiler: &Compiler, command: &str, allow: bool) -> SkewVerdict {
    let (Some(skew), Some(ver), Some(path)) = (compiler.skew, compiler.version, &compiler.path)
    else {
        return SkewVerdict::Run;
    };
    let driver = crate::DRIVER_VERSION;
    let which = format!("`bynkc` {ver} ({})", path.display());
    let remedy = "install a `bynkc` matching `bynk`, point BYNK_BYNKC at one, or run `bynk doctor`";
    match skew {
        Skew::Match => SkewVerdict::Run,
        Skew::Minor => SkewVerdict::Warn(format!(
            "bynk {command}: warning: {which} is a different minor version from bynk {driver} — \
             it may check this code differently. To align: {remedy}."
        )),
        Skew::Major if allow => SkewVerdict::Warn(format!(
            "bynk {command}: warning: {which} is a different major version from bynk {driver}; \
             running it anyway (skew allowed)."
        )),
        Skew::Major => SkewVerdict::Refuse(format!(
            "bynk {command}: {which} is a different major version from bynk {driver}, so the two \
             do not share a contract. Refusing to run it. To align: {remedy}. To run it anyway, \
             pass --allow-skew (`bynk test`) or set {ALLOW_SKEW_ENV}=1."
        )),
    }
}

/// Print `verdict`'s message (if any) and say whether to go ahead.
pub fn apply_skew_verdict(verdict: SkewVerdict) -> bool {
    match verdict {
        SkewVerdict::Run => true,
        SkewVerdict::Warn(msg) => {
            eprintln!("{msg}");
            true
        }
        SkewVerdict::Refuse(msg) => {
            eprintln!("{msg}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An in-memory toolbox: `files` are exact (dir, name) pairs that
    /// resolve; nothing is on PATH unless listed in `on_path`.
    struct FakeToolbox {
        files: Vec<(PathBuf, String)>,
        on_path: Vec<String>,
    }

    impl Toolbox for FakeToolbox {
        fn on_path(&self, tool: &str) -> Option<PathBuf> {
            self.on_path
                .iter()
                .any(|t| t == tool)
                .then(|| PathBuf::from("/usr/bin").join(tool))
        }
        fn in_dir(&self, dir: &Path, tool: &str) -> Option<PathBuf> {
            self.files
                .iter()
                .any(|(d, n)| d == dir && n == tool)
                .then(|| dir.join(tool))
        }
        fn version(&self, _path: &Path) -> Option<Version> {
            None
        }
        fn npx_available(&self) -> bool {
            false
        }
    }

    #[test]
    fn missing_override_resolves_to_none() {
        // A typo'd override must surface as unresolved — not report Ok and
        // then fail at spawn (#514).
        let tb = FakeToolbox {
            files: vec![],
            on_path: vec!["bynkc".into()],
        };
        let (path, origin) = locate(&tb, Some(Path::new("/opt/missing/bynkc")), None);
        assert_eq!(path, None);
        assert_eq!(origin, Some(Origin::Override));
    }

    #[test]
    fn empty_override_is_unset() {
        // `BYNK_BYNKC=""` must not resolve `./bynkc` from the CWD.
        let tb = FakeToolbox {
            files: vec![(PathBuf::from("."), "bynkc".into())],
            on_path: vec!["bynkc".into()],
        };
        let (path, origin) = locate(&tb, Some(Path::new("")), None);
        assert_eq!(origin, Some(Origin::Path));
        assert_eq!(path, Some(PathBuf::from("/usr/bin/bynkc")));
    }

    #[test]
    fn override_never_resolves_by_stem() {
        // `/dir/bynkc.backup` names one binary; stem-stripping used to pick
        // the *different* `/dir/bynkc` silently.
        let tb = FakeToolbox {
            files: vec![(PathBuf::from("/dir"), "bynkc".into())],
            on_path: vec![],
        };
        let (path, origin) = locate(&tb, Some(Path::new("/dir/bynkc.backup")), None);
        assert_eq!(path, None, "the named backup binary does not exist");
        assert_eq!(origin, Some(Origin::Override));

        // And the exact name resolves when present.
        let tb = FakeToolbox {
            files: vec![(PathBuf::from("/dir"), "bynkc.backup".into())],
            on_path: vec![],
        };
        let (path, _) = locate(&tb, Some(Path::new("/dir/bynkc.backup")), None);
        assert_eq!(path, Some(PathBuf::from("/dir/bynkc.backup")));
    }

    #[test]
    fn skew_classification() {
        let v = |a, b, c| Version {
            major: a,
            minor: b,
            patch: c,
        };
        assert_eq!(Skew::classify(v(0, 46, 0), v(0, 46, 0)), Skew::Match);
        // patch drift is wire-compatible
        assert_eq!(Skew::classify(v(0, 46, 0), v(0, 46, 3)), Skew::Match);
        assert_eq!(Skew::classify(v(0, 46, 0), v(0, 44, 0)), Skew::Minor);
        assert_eq!(Skew::classify(v(1, 0, 0), v(0, 46, 0)), Skew::Major);
    }

    fn skewed(skew: Skew) -> Compiler {
        Compiler {
            path: Some(PathBuf::from("/usr/bin/bynkc")),
            origin: Some(Origin::Path),
            version: Some(Version {
                major: 0,
                minor: 1,
                patch: 0,
            }),
            skew: Some(skew),
        }
    }

    /// #1675: match runs silently; minor skew warns and runs.
    #[test]
    fn minor_skew_warns_and_runs() {
        assert_eq!(
            skew_verdict(&skewed(Skew::Match), "test", false),
            SkewVerdict::Run
        );
        let SkewVerdict::Warn(msg) = skew_verdict(&skewed(Skew::Minor), "test", false) else {
            panic!("minor skew must warn");
        };
        assert!(msg.contains("different minor version"), "{msg}");
        assert!(msg.contains("/usr/bin/bynkc"), "names the binary: {msg}");
    }

    /// #1675: major skew refuses, naming both overrides; allowed, it warns.
    #[test]
    fn major_skew_refuses_unless_allowed() {
        let SkewVerdict::Refuse(msg) = skew_verdict(&skewed(Skew::Major), "test", false) else {
            panic!("major skew must refuse");
        };
        assert!(
            msg.contains("--allow-skew") && msg.contains(ALLOW_SKEW_ENV),
            "{msg}"
        );
        let SkewVerdict::Warn(msg) = skew_verdict(&skewed(Skew::Major), "test", true) else {
            panic!("allowed major skew must warn, not refuse");
        };
        assert!(msg.contains("running it anyway"), "{msg}");
    }

    /// No compiler, or an unreadable version: nothing to judge, so run (the
    /// missing-compiler case is reported by the caller).
    #[test]
    fn unknown_skew_runs() {
        let mut c = skewed(Skew::Major);
        c.skew = None;
        assert_eq!(skew_verdict(&c, "test", false), SkewVerdict::Run);
    }
}
