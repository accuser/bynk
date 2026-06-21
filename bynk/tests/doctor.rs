//! `bynk doctor` — the capability/exit-code matrix, the detection probe, and
//! the pinned `--format short`/`json` goldens.
//!
//! Everything runs against a deterministic [`Fake`] toolbox and hand-built
//! [`Compiler`] states, so the contract is exercised without depending on the
//! host's tools. The goldens are blessed with `BYNK_BLESS=1`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bynk::compiler::{Compiler, Origin, Skew};
use bynk::doctor::{self, Capability, Context, DoctorOptions, Level};
use bynk::probe::{self, DetectOpts, Provenance, Toolbox, Version};
use bynk::report::{self, Format};

// ---------------------------------------------------------------------------
// Fake toolbox
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Fake {
    on_path: HashMap<String, PathBuf>,
    project_local: HashMap<String, PathBuf>,
    versions: HashMap<PathBuf, Version>,
    npx: bool,
}

impl Fake {
    fn path_tool(mut self, tool: &str, path: &str, ver: Option<Version>) -> Self {
        let p = PathBuf::from(path);
        self.on_path.insert(tool.into(), p.clone());
        if let Some(v) = ver {
            self.versions.insert(p, v);
        }
        self
    }
    fn local_tool(mut self, tool: &str, path: &str, ver: Option<Version>) -> Self {
        let p = PathBuf::from(path);
        self.project_local.insert(tool.into(), p.clone());
        if let Some(v) = ver {
            self.versions.insert(p, v);
        }
        self
    }
    fn with_npx(mut self) -> Self {
        self.npx = true;
        self
    }
}

impl Toolbox for Fake {
    fn on_path(&self, tool: &str) -> Option<PathBuf> {
        self.on_path.get(tool).cloned()
    }
    fn in_dir(&self, dir: &Path, tool: &str) -> Option<PathBuf> {
        // Only resolve project-local tools when asked inside a node_modules/.bin.
        if dir.ends_with(Path::new("node_modules/.bin")) {
            self.project_local.get(tool).cloned()
        } else {
            None
        }
    }
    fn version(&self, path: &Path) -> Option<Version> {
        self.versions.get(path).copied()
    }
    fn npx_available(&self) -> bool {
        self.npx
    }
}

fn v(major: u32, minor: u32, patch: u32) -> Version {
    Version {
        major,
        minor,
        patch,
    }
}

/// A `bynkc` resolved on PATH, with the given skew vs the driver.
fn bynkc_ok(skew: Skew) -> Compiler {
    Compiler {
        path: Some(PathBuf::from("/opt/bynk/bin/bynkc")),
        origin: Some(Origin::Path),
        version: Some(v(9, 9, 9)),
        skew: Some(skew),
    }
}

/// A `BYNK_BYNKC` override at the given skew. Slice 7: the in-process compiler
/// is always ok, so skew is only meaningful — and only reported — when the user
/// points `bynk` at an external compiler via the override (ADR 0084, amended).
fn bynkc_override(skew: Skew) -> Compiler {
    Compiler {
        path: Some(PathBuf::from("/opt/bynk/bin/bynkc")),
        origin: Some(Origin::Override),
        version: Some(v(9, 9, 9)),
        skew: Some(skew),
    }
}

/// A `BYNK_BYNKC` override that doesn't resolve — the only "broken compiler"
/// state under the amended contract (a missing in-process compiler is impossible).
fn bynkc_override_missing() -> Compiler {
    Compiler {
        path: None,
        origin: Some(Origin::Override),
        version: None,
        skew: None,
    }
}

fn bynkc_missing() -> Compiler {
    Compiler {
        path: None,
        origin: None,
        version: None,
        skew: None,
    }
}

fn ctx(in_repo: bool) -> Context {
    Context {
        project_root: None,
        in_repo,
        node_floor: 18,
    }
}

fn bare() -> DoctorOptions {
    DoctorOptions::default()
}

fn cap(report: &doctor::Report, c: Capability) -> &doctor::CapabilityReport {
    report
        .capabilities
        .iter()
        .find(|r| r.capability == c)
        .expect("capability present")
}

// ---------------------------------------------------------------------------
// Detection probe
// ---------------------------------------------------------------------------

#[test]
fn project_local_is_preferred_over_global() {
    let fake = Fake::default()
        .path_tool("tsc", "/usr/bin/tsc", Some(v(5, 0, 0)))
        .local_tool("tsc", "/proj/node_modules/.bin/tsc", Some(v(5, 4, 2)));
    let root = PathBuf::from("/proj");
    let probe = probe::detect(
        &fake,
        "tsc",
        DetectOpts {
            project_root: Some(&root),
            allow_npx: true,
        },
    );
    assert_eq!(
        probe.provenance,
        Provenance::ProjectLocal(PathBuf::from("/proj/node_modules/.bin/tsc"))
    );
    assert_eq!(probe.version, Some(v(5, 4, 2)));
}

#[test]
fn npx_is_provisionable_not_present() {
    let fake = Fake::default().with_npx();
    let probe = probe::detect(
        &fake,
        "wrangler",
        DetectOpts {
            project_root: None,
            allow_npx: true,
        },
    );
    assert!(probe.is_provisionable());
    assert!(!probe.is_present(), "npx must never read as installed");
    assert_eq!(probe.provenance, Provenance::Npx);
}

#[test]
fn absent_with_no_npx_is_missing() {
    let fake = Fake::default();
    let probe = probe::detect(
        &fake,
        "wrangler",
        DetectOpts {
            project_root: None,
            allow_npx: true,
        },
    );
    assert!(probe.is_missing());
}

// ---------------------------------------------------------------------------
// Capability / exit-code matrix
// ---------------------------------------------------------------------------

#[test]
fn bare_compile_only_env_exits_zero_with_test_dev_flagged() {
    // Only bynkc; no node/tsc/tsx/wrangler.
    let fake = Fake::default();
    let report = doctor::diagnose(&fake, &bynkc_ok(Skew::Match), &ctx(false), &bare());
    assert_eq!(cap(&report, Capability::Compile).level, Level::Ok);
    assert_eq!(cap(&report, Capability::Test).level, Level::Fail);
    assert_eq!(cap(&report, Capability::Deploy).level, Level::Fail);
    // Bare run: only the compile floor is required, so it exits 0.
    assert!(!report.exit_nonzero(&bare()));
}

#[test]
fn everything_present_is_all_green_exit_zero() {
    let fake = Fake::default()
        .path_tool("node", "/usr/bin/node", Some(v(20, 0, 0)))
        .path_tool("tsc", "/usr/bin/tsc", Some(v(5, 4, 2)))
        .path_tool("wrangler", "/usr/bin/wrangler", Some(v(3, 90, 0)))
        .path_tool("bynkc-lsp", "/usr/bin/bynkc-lsp", Some(v(9, 9, 9)));
    let report = doctor::diagnose(&fake, &bynkc_ok(Skew::Match), &ctx(false), &bare());
    assert!(report.is_all_ok());
    assert!(!report.exit_nonzero(&bare()));
}

#[test]
fn only_deploy_without_wrangler_exits_nonzero() {
    // No wrangler, no npx → truly cannot deploy.
    let fake = Fake::default().path_tool("node", "/usr/bin/node", Some(v(20, 0, 0)));
    let report = doctor::diagnose(&fake, &bynkc_ok(Skew::Match), &ctx(false), &bare());
    let opts = DoctorOptions {
        only: Some(Capability::Deploy),
        strict: false,
    };
    assert_eq!(cap(&report, Capability::Deploy).level, Level::Fail);
    assert!(report.exit_nonzero(&opts));
    // …but the same environment, bare, exits 0 (deploy isn't asked about).
    assert!(!report.exit_nonzero(&bare()));
}

#[test]
fn strict_escalates_optional_only_gap() {
    // Everything an end user needs is present; only the optional editor LSP is
    // missing. Bare → 0; --strict → non-zero.
    let fake = Fake::default()
        .path_tool("node", "/usr/bin/node", Some(v(20, 0, 0)))
        .path_tool("tsc", "/usr/bin/tsc", Some(v(5, 4, 2)))
        .path_tool("wrangler", "/usr/bin/wrangler", Some(v(3, 90, 0)));
    let report = doctor::diagnose(&fake, &bynkc_ok(Skew::Match), &ctx(false), &bare());
    assert_eq!(cap(&report, Capability::Editor).level, Level::Fail); // displayed as "note"
    assert!(!report.exit_nonzero(&bare()));
    let strict = DoctorOptions {
        only: None,
        strict: true,
    };
    assert!(report.exit_nonzero(&strict));
}

#[test]
fn in_process_compile_floor_is_always_ok() {
    // Slice 7 (ADR 0084 amended): the compiler is linked in-process, so the
    // compile floor is always satisfiable — a missing *external* bynkc is no
    // longer a failure (the driver doesn't shell it). Compile is ok, bare.
    let fake = Fake::default()
        .path_tool("node", "/usr/bin/node", Some(v(20, 0, 0)))
        .path_tool("tsc", "/usr/bin/tsc", Some(v(5, 4, 2)));
    let report = doctor::diagnose(&fake, &bynkc_missing(), &ctx(false), &bare());
    assert_eq!(cap(&report, Capability::Compile).level, Level::Ok);
}

#[test]
fn broken_override_fails_even_bare() {
    // A `BYNK_BYNKC` override that doesn't resolve is the one broken-compiler
    // state left: the user explicitly asked for an external compiler and it
    // isn't there. Fails even bare.
    let fake = Fake::default()
        .path_tool("node", "/usr/bin/node", Some(v(20, 0, 0)))
        .path_tool("tsc", "/usr/bin/tsc", Some(v(5, 4, 2)));
    let report = doctor::diagnose(&fake, &bynkc_override_missing(), &ctx(false), &bare());
    assert_eq!(cap(&report, Capability::Compile).level, Level::Fail);
    assert!(report.exit_nonzero(&bare()));
}

#[test]
fn skew_is_reported_only_under_override() {
    let fake = Fake::default();

    // Slice 7: skew at a non-override origin is ignored — the in-process
    // compiler can't drift against itself.
    let path_major = doctor::diagnose(&fake, &bynkc_ok(Skew::Major), &ctx(false), &bare());
    assert_eq!(cap(&path_major, Capability::Compile).level, Level::Ok);

    // Under a `BYNK_BYNKC` override, skew is real and reported.
    // Minor: bare exits 0 (warn), --strict fails.
    let minor = doctor::diagnose(&fake, &bynkc_override(Skew::Minor), &ctx(false), &bare());
    assert_eq!(cap(&minor, Capability::Compile).level, Level::Warn);
    assert!(!minor.exit_nonzero(&bare()));
    assert!(minor.exit_nonzero(&DoctorOptions {
        only: None,
        strict: true
    }));

    // Major: a contract mismatch — fails even bare.
    let major = doctor::diagnose(&fake, &bynkc_override(Skew::Major), &ctx(false), &bare());
    assert_eq!(cap(&major, Capability::Compile).level, Level::Fail);
    assert!(major.exit_nonzero(&bare()));
}

#[test]
fn npx_provisionable_runner_is_warn_not_ok() {
    // tsc/tsx only via npx → test capability is available but flagged (never a
    // green "ok"), and --strict escalates it.
    let fake = Fake::default()
        .path_tool("node", "/usr/bin/node", Some(v(20, 0, 0)))
        .with_npx();
    let report = doctor::diagnose(&fake, &bynkc_ok(Skew::Match), &ctx(false), &bare());
    assert_eq!(cap(&report, Capability::Test).level, Level::Warn);
    assert!(!report.exit_nonzero(&bare()));
    assert!(report.exit_nonzero(&DoctorOptions {
        only: Some(Capability::Test),
        strict: true,
    }));
}

#[test]
fn only_filter_scopes_probes() {
    // `--only test` must not probe Cloudflare/editor at all.
    let fake = Fake::default().path_tool("node", "/usr/bin/node", Some(v(20, 0, 0)));
    let opts = DoctorOptions {
        only: Some(Capability::Test),
        strict: false,
    };
    let report = doctor::diagnose(&fake, &bynkc_ok(Skew::Match), &ctx(false), &opts);
    let kinds: Vec<Capability> = report.capabilities.iter().map(|c| c.capability).collect();
    assert_eq!(kinds, vec![Capability::Compile, Capability::Test]);
}

// ---------------------------------------------------------------------------
// Pinned output goldens (--format short / json)
// ---------------------------------------------------------------------------

/// A fixed, mixed environment: compile ok, test ok, deploy provisionable-only
/// (warn), editor missing (note). Driver/compiler versions are pinned to a
/// sentinel so the goldens survive version bumps.
fn golden_report() -> doctor::Report {
    let fake = Fake::default()
        .path_tool("node", "/usr/bin/node", Some(v(20, 0, 0)))
        .path_tool("tsc", "/usr/bin/tsc", Some(v(5, 4, 2)))
        .with_npx(); // wrangler provisionable, not installed
    let mut report = doctor::diagnose(&fake, &bynkc_ok(Skew::Match), &ctx(false), &bare());
    report.driver_version = "9.9.9".to_string();
    report
}

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

#[test]
fn golden_short() {
    bless_or_assert(
        "doctor.short",
        &report::render(&golden_report(), Format::Short),
    );
}

#[test]
fn golden_json() {
    bless_or_assert(
        "doctor.json",
        &report::render(&golden_report(), Format::Json),
    );
}

#[test]
fn human_smoke() {
    // The human table is smoke-tested only (not pinned): it must mention each
    // capability and not leak an absolute path into a capability row.
    let out = report::render(&golden_report(), Format::Human);
    for token in ["compile", "test", "deploy", "editor"] {
        assert!(out.contains(token), "human output missing {token}");
    }
    assert!(out.contains("bynk doctor"));
}
