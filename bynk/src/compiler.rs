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
    if let Some(ovr) = override_path {
        // An explicit override is taken as-is when it resolves; we do not fall
        // through on a bad override, so a typo surfaces rather than silently
        // picking a different compiler.
        if let Some(p) = tb.in_dir(ovr.parent().unwrap_or(Path::new(".")), file_stem(ovr)) {
            return (Some(p), Some(Origin::Override));
        }
        return (Some(ovr.to_path_buf()), Some(Origin::Override));
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

fn file_stem(p: &Path) -> &str {
    p.file_stem().and_then(|s| s.to_str()).unwrap_or("bynkc")
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
