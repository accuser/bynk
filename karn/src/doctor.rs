//! `karn doctor` — the capability model, the checks, and the exit-code
//! contract.
//!
//! Probes are **grouped by the capability they unlock**, not listed flat, so a
//! compile-only user is never told they are "unhealthy" for lacking `wrangler`.
//! The exit-code contract turns on *what an invocation asks about* (ADR: the
//! doctor output / exit-code contract):
//!
//! - **Bare `karn doctor`** is informational. It surveys everything but treats
//!   only the *compile floor* (`karnc` resolvable and not majorly skewed) as
//!   required, so it exits `0` even with `test`/`dev` unavailable.
//! - **`--only <capability>`** promotes that capability's tools to required.
//! - **`--strict`** promotes *all* warnings (optional gaps, `npx`
//!   provisionability, minor skew) to failures, for an all-green CI gate.

use std::path::PathBuf;

use crate::compiler::{Compiler, Skew};
use crate::probe::{self, DetectOpts, Probe, Toolbox};

/// A unit of work a user might want to do, and the tools it needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    /// `karnc` compile / check / fmt. Always satisfiable if `karnc` resolved;
    /// also the home of the driver↔compiler skew check.
    Compile,
    /// `karn test` — Node and one of `tsc`/`tsx` (the runner ladder).
    Test,
    /// `dev` / deploy to Cloudflare — Node and `wrangler`.
    Deploy,
    /// Editor support — `karnc-lsp`. Optional; never a failure (except strict).
    Editor,
    /// Build Karn from source — a Rust toolchain. Contributor-only; reported
    /// only inside the Karn repo.
    BuildFromSource,
}

impl Capability {
    pub fn token(self) -> &'static str {
        match self {
            Capability::Compile => "compile",
            Capability::Test => "test",
            Capability::Deploy => "deploy",
            Capability::Editor => "editor",
            Capability::BuildFromSource => "build",
        }
    }

    /// Optional capabilities never fail a run on their own — they note, and
    /// `--strict` escalates.
    pub fn is_optional(self) -> bool {
        matches!(self, Capability::Editor | Capability::BuildFromSource)
    }
}

/// Health of a single row or a whole capability. Ordered: `Ok < Warn < Fail`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Ok,
    Warn,
    Fail,
}

/// One rendered line under a capability: a tool (or an any-of group like
/// `tsc | tsx`), its health, a human detail, and a remedy when it is not `Ok`.
#[derive(Debug, Clone)]
pub struct Row {
    pub label: String,
    pub level: Level,
    pub detail: String,
    pub remedy: Option<String>,
}

/// A capability and its rows, with the aggregated health.
#[derive(Debug, Clone)]
pub struct CapabilityReport {
    pub capability: Capability,
    pub optional: bool,
    pub rows: Vec<Row>,
    pub level: Level,
}

/// The whole `doctor` result.
#[derive(Debug, Clone)]
pub struct Report {
    pub driver_version: String,
    pub compiler: Compiler,
    pub capabilities: Vec<CapabilityReport>,
}

/// User-facing knobs.
#[derive(Debug, Clone, Default)]
pub struct DoctorOptions {
    /// Scope the gate to one capability (promotes its tools to required).
    pub only: Option<Capability>,
    /// Escalate every warning to a failure.
    pub strict: bool,
}

/// Environment facts the caller supplies (real values in `main`, fixed values
/// in tests).
#[derive(Debug, Clone)]
pub struct Context {
    /// Discovered project root (`karn.toml`), for project-local resolution.
    pub project_root: Option<PathBuf>,
    /// Whether to include the contributor `build` capability.
    pub in_repo: bool,
    /// Minimum supported Node major (single-sourced from `karnc`).
    pub node_floor: u32,
}

impl Report {
    /// Should the process exit non-zero, given the options?
    ///
    /// Non-zero iff a *required* capability has a hard failure, or — under
    /// `--strict` — any capability is less than `Ok`. The compile floor is
    /// always required; `--only <cap>` adds that capability.
    pub fn exit_nonzero(&self, opts: &DoctorOptions) -> bool {
        for cap in &self.capabilities {
            let required =
                cap.capability == Capability::Compile || opts.only == Some(cap.capability);
            if required && cap.level == Level::Fail {
                return true;
            }
        }
        if opts.strict && self.capabilities.iter().any(|c| c.level != Level::Ok) {
            return true;
        }
        false
    }

    /// One-word overall summary for the human header.
    pub fn is_all_ok(&self) -> bool {
        self.capabilities.iter().all(|c| c.level == Level::Ok)
    }
}

/// Run the checks against a toolbox and a resolved compiler.
pub fn diagnose(
    tb: &dyn Toolbox,
    compiler: &Compiler,
    ctx: &Context,
    opts: &DoctorOptions,
) -> Report {
    let root = ctx.project_root.as_deref();
    let mut capabilities = vec![compile_report(compiler)];

    // Only build the capability the user scoped to (plus the always-on compile
    // floor), so `--only test` doesn't probe Cloudflare. With no filter, build
    // them all.
    let want = |cap: Capability| opts.only.is_none() || opts.only == Some(cap);

    if want(Capability::Test) {
        let node = detect_node(tb, root, ctx.node_floor);
        let runner = detect_runner(tb, root);
        capabilities.push(capability(Capability::Test, vec![node, runner]));
    }
    if want(Capability::Deploy) {
        let node = detect_node(tb, root, ctx.node_floor);
        let wrangler = detect_npm_tool(tb, root, "wrangler", "npm install -g wrangler");
        capabilities.push(capability(Capability::Deploy, vec![node, wrangler]));
    }
    if want(Capability::Editor) {
        let lsp = detect_plain(
            tb,
            "karnc-lsp",
            "install karnc-lsp (or download from releases)",
        );
        capabilities.push(capability(Capability::Editor, vec![lsp]));
    }
    if ctx.in_repo && want(Capability::BuildFromSource) {
        let cargo = detect_plain(tb, "cargo", "install Rust via https://rustup.rs");
        capabilities.push(capability(Capability::BuildFromSource, vec![cargo]));
    }

    Report {
        driver_version: crate::DRIVER_VERSION.to_string(),
        compiler: compiler.clone(),
        capabilities,
    }
}

/// Compile/check/fmt: just `karnc`, plus the skew check.
fn compile_report(compiler: &Compiler) -> CapabilityReport {
    let row = match (&compiler.path, compiler.version, compiler.skew) {
        (None, _, _) => Row {
            label: "karnc".into(),
            level: Level::Fail,
            detail: "not found (PATH, sibling, or $KARN_KARNC)".into(),
            remedy: Some("install karnc, or set KARN_KARNC to its path".into()),
        },
        (Some(path), version, skew) => {
            let origin = compiler.origin.map(|o| o.token()).unwrap_or("path");
            let ver = version
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unknown".into());
            let (level, detail, remedy) = match skew {
                Some(Skew::Major) => (
                    Level::Fail,
                    format!("{ver} ({origin}) — major skew vs driver"),
                    Some("align karn and karnc versions".to_string()),
                ),
                Some(Skew::Minor) => (
                    Level::Warn,
                    format!("{ver} ({origin}) — minor skew vs driver"),
                    Some("align karn and karnc versions".to_string()),
                ),
                _ => (Level::Ok, format!("{ver} ({origin})"), None),
            };
            let _ = path;
            Row {
                label: "karnc".into(),
                level,
                detail,
                remedy,
            }
        }
    };
    let level = row.level;
    CapabilityReport {
        capability: Capability::Compile,
        optional: false,
        rows: vec![row],
        level,
    }
}

/// Aggregate a capability from its rows (worst row wins).
fn capability(cap: Capability, rows: Vec<Row>) -> CapabilityReport {
    let level = rows.iter().map(|r| r.level).max().unwrap_or(Level::Ok);
    CapabilityReport {
        capability: cap,
        optional: cap.is_optional(),
        rows,
        level,
    }
}

fn detect_node(tb: &dyn Toolbox, root: Option<&std::path::Path>, floor: u32) -> Row {
    // A runtime is never npx-provisionable.
    let probe = probe::detect(
        tb,
        "node",
        DetectOpts {
            project_root: root,
            allow_npx: false,
        },
    );
    let remedy = format!("install Node.js ≥ {floor} from https://nodejs.org");
    if probe.is_missing() {
        return Row {
            label: "node".into(),
            level: Level::Fail,
            detail: "missing".into(),
            remedy: Some(remedy),
        };
    }
    let below = probe.version.map(|v| v.major < floor).unwrap_or(false);
    if below {
        let v = probe.version.unwrap();
        return Row {
            label: "node".into(),
            level: Level::Warn,
            detail: format!("v{v} below floor (≥ {floor})"),
            remedy: Some(remedy),
        };
    }
    Row {
        label: "node".into(),
        level: Level::Ok,
        detail: present_detail(&probe),
        remedy: None,
    }
}

/// The `tsc | tsx` runner requirement — satisfied by the *better* of the two.
fn detect_runner(tb: &dyn Toolbox, root: Option<&std::path::Path>) -> Row {
    let tsc = probe::detect(
        tb,
        "tsc",
        DetectOpts {
            project_root: root,
            allow_npx: true,
        },
    );
    let tsx = probe::detect(
        tb,
        "tsx",
        DetectOpts {
            project_root: root,
            allow_npx: true,
        },
    );
    let best = pick_better(&tsc, &tsx);
    let remedy = "npm install -g tsx (or: npm install -g typescript)".to_string();
    match best {
        Some(p) if p.is_present() => Row {
            label: "tsc | tsx".into(),
            level: Level::Ok,
            detail: format!("{} {}", p.tool, present_detail(p)),
            remedy: None,
        },
        Some(p) => Row {
            // provisionable via npx
            label: "tsc | tsx".into(),
            level: Level::Warn,
            detail: format!("{} provisionable via npx (not installed)", p.tool),
            remedy: Some(remedy),
        },
        None => Row {
            label: "tsc | tsx".into(),
            level: Level::Fail,
            detail: "missing".into(),
            remedy: Some(remedy),
        },
    }
}

fn detect_npm_tool(
    tb: &dyn Toolbox,
    root: Option<&std::path::Path>,
    tool: &str,
    remedy: &str,
) -> Row {
    let probe = probe::detect(
        tb,
        tool,
        DetectOpts {
            project_root: root,
            allow_npx: true,
        },
    );
    npm_row(tool, &probe, remedy)
}

fn detect_plain(tb: &dyn Toolbox, tool: &str, remedy: &str) -> Row {
    let probe = probe::detect(
        tb,
        tool,
        DetectOpts {
            project_root: None,
            allow_npx: false,
        },
    );
    if probe.is_present() {
        Row {
            label: tool.into(),
            level: Level::Ok,
            detail: present_detail(&probe),
            remedy: None,
        }
    } else {
        Row {
            label: tool.into(),
            level: Level::Fail,
            detail: "missing".into(),
            remedy: Some(remedy.into()),
        }
    }
}

fn npm_row(tool: &str, probe: &Probe, remedy: &str) -> Row {
    if probe.is_present() {
        Row {
            label: tool.into(),
            level: Level::Ok,
            detail: present_detail(probe),
            remedy: None,
        }
    } else if probe.is_provisionable() {
        Row {
            label: tool.into(),
            level: Level::Warn,
            detail: "provisionable via npx (not installed)".into(),
            remedy: Some(remedy.into()),
        }
    } else {
        Row {
            label: tool.into(),
            level: Level::Fail,
            detail: "missing".into(),
            remedy: Some(remedy.into()),
        }
    }
}

/// `path`/`project-local` beats `npx` beats `missing`; among installed, prefer
/// the first argument (caller order).
fn pick_better<'a>(a: &'a Probe, b: &'a Probe) -> Option<&'a Probe> {
    fn rank(p: &Probe) -> u8 {
        if p.is_present() {
            2
        } else if p.is_provisionable() {
            1
        } else {
            0
        }
    }
    let (ra, rb) = (rank(a), rank(b));
    if ra == 0 && rb == 0 {
        None
    } else if ra >= rb {
        Some(a)
    } else {
        Some(b)
    }
}

fn present_detail(probe: &Probe) -> String {
    let ver = probe
        .version
        .map(|v| format!("v{v}"))
        .unwrap_or_else(|| "installed".into());
    format!("{ver} ({})", probe.provenance.token())
}
