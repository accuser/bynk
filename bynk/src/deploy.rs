//! `bynk deploy` — provision persistent Cloudflare identity, then publish.
//!
//! The generated `wrangler.toml` is deliberately disposable. This module owns
//! the small, committed `bynk.deploy.lock` ledger and materialises its KV id
//! into a freshly compiled worker immediately before Wrangler sees it.

use std::collections::BTreeMap;
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::process::{ExitCode, Stdio};

use serde::{Deserialize, Serialize};

use crate::compiler::Compiler;
use crate::dev;
use crate::doctor::{self, Capability, Context, DoctorOptions, Report};
use crate::probe::{self, DetectOpts, Provenance, Toolbox};
use crate::report::{self, Format};
use crate::shell::exit_status_byte;

const LOCK_FILE: &str = "bynk.deploy.lock";
use bynk_emit::emitter::wrangler::KV_NAMESPACE_ID_PLACEHOLDER;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DeployFormat {
    #[default]
    Short,
    Json,
}

#[derive(Debug, Clone, Default)]
pub struct DeployOptions {
    pub dry_run: bool,
    pub format: DeployFormat,
    pub yes: bool,
    pub wrangler_args: Vec<String>,
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
struct DeployLock {
    #[serde(default = "lock_version")]
    version: u32,
    #[serde(default)]
    environments: BTreeMap<String, Environment>,
}

fn lock_version() -> u32 {
    1
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
struct Environment {
    #[serde(default)]
    kv: BTreeMap<String, KvNamespace>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct KvNamespace {
    id: String,
}

#[derive(Debug, Serialize)]
struct Plan<'a> {
    environment: &'static str,
    worker: &'a str,
    kv: Option<PlanKv<'a>>,
    deploy: bool,
}

#[derive(Debug, Serialize)]
struct PlanKv<'a> {
    action: &'static str,
    namespace: &'a str,
}

/// Run the slice-0 single-context deployment pipeline.
pub fn run(
    tb: &dyn Toolbox,
    compiler: &Compiler,
    project_root: &Path,
    node_floor: u32,
    opts: &DeployOptions,
) -> ExitCode {
    let preflight_opts = DoctorOptions {
        only: Some(Capability::Deploy),
        strict: false,
    };
    let report = doctor::diagnose(
        tb,
        compiler,
        &Context {
            project_root: Some(project_root.to_path_buf()),
            in_repo: false,
            node_floor,
        },
        &preflight_opts,
    );
    if report.exit_nonzero(&preflight_opts) {
        eprint!("{}", preflight_failure_message(&report));
        return ExitCode::FAILURE;
    }

    let build_dir = project_root.join(".bynk").join("deploy");
    if let Err(e) = dev::prepare_build_dir(project_root, &build_dir) {
        eprintln!("bynk: could not prepare build directory: {e}");
        return ExitCode::FAILURE;
    }
    if !dev::compile_once(compiler, project_root, &build_dir) {
        return ExitCode::FAILURE;
    }
    let workers = dev::discover_workers(&build_dir.join("workers"));
    let worker = match dev::select_context(&workers, None) {
        Ok(worker) => worker,
        Err(e) => {
            eprintln!("bynk: deploy requires one context — {e}");
            return ExitCode::FAILURE;
        }
    };
    let worker_dir = build_dir.join("workers").join(&worker);
    let config = worker_dir.join("wrangler.toml");
    let needs_kv = match std::fs::read_to_string(&config) {
        Ok(text) => text.contains(KV_NAMESPACE_ID_PLACEHOLDER),
        Err(e) => {
            eprintln!("bynk: could not read generated configuration: {e}");
            return ExitCode::FAILURE;
        }
    };
    let lock_path = project_root.join(LOCK_FILE);
    let mut lock = match read_lock(&lock_path) {
        Ok(lock) => lock,
        Err(e) => {
            eprintln!("bynk: could not read {}: {e}", lock_path.display());
            return ExitCode::FAILURE;
        }
    };
    let recorded = lock
        .environments
        .get("default")
        .and_then(|env| env.kv.get(&worker))
        .map(|kv| kv.id.as_str());
    let plan = derive_plan(&worker, needs_kv, recorded);
    print_plan(&plan, opts.format);
    if opts.dry_run {
        return ExitCode::SUCCESS;
    }

    if should_refuse_unrecorded_ci(needs_kv, recorded, is_ci()) {
        eprintln!(
            "bynk: KV namespace for `{worker}` is unrecorded; provision locally first and commit {LOCK_FILE}"
        );
        return ExitCode::FAILURE;
    }
    let probe = probe::detect(
        tb,
        "wrangler",
        DetectOpts {
            project_root: Some(project_root),
            allow_npx: true,
        },
    );
    if !whoami(&probe.provenance) {
        eprintln!(
            "bynk: Cloudflare authentication is unavailable; run `wrangler login` or set CLOUDFLARE_API_TOKEN"
        );
        return ExitCode::FAILURE;
    }
    if !confirm(opts.yes) {
        return ExitCode::FAILURE;
    }
    let kv_id = if needs_kv {
        match recorded {
            Some(id) => id.to_owned(),
            None => match create_kv(&probe.provenance, &worker, project_root) {
                Ok(id) => {
                    lock.environments
                        .entry("default".into())
                        .or_default()
                        .kv
                        .insert(worker.clone(), KvNamespace { id: id.clone() });
                    if let Err(e) = write_lock(&lock_path, &lock) {
                        eprintln!(
                            "bynk: created KV namespace but could not record it in {}: {e}",
                            lock_path.display()
                        );
                        return ExitCode::FAILURE;
                    }
                    id
                }
                Err(e) => {
                    eprintln!("bynk: could not create KV namespace: {e}");
                    return ExitCode::FAILURE;
                }
            },
        }
    } else {
        String::new()
    };
    if needs_kv && !materialise_kv_id(&config, &kv_id) {
        eprintln!("bynk: could not materialise KV namespace id into generated configuration");
        return ExitCode::FAILURE;
    }
    let Some(mut command) = dev::wrangler_command(&probe.provenance, "deploy") else {
        eprintln!("bynk: wrangler not found");
        return ExitCode::FAILURE;
    };
    command.current_dir(&worker_dir).args(&opts.wrangler_args);
    match command.status() {
        Ok(status) => ExitCode::from(exit_status_byte(&status)),
        Err(e) => {
            eprintln!("bynk: could not run wrangler deploy: {e}");
            ExitCode::FAILURE
        }
    }
}

pub fn preflight_failure_message(report: &Report) -> String {
    format!(
        "bynk: environment not ready for `deploy` — see below.\n\n{}",
        report::render(report, Format::Human)
    )
}

fn read_lock(path: &Path) -> Result<DeployLock, String> {
    if !path.exists() {
        return Ok(DeployLock {
            version: lock_version(),
            ..Default::default()
        });
    }
    let lock: DeployLock =
        toml::from_str(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if lock.version != lock_version() {
        return Err(format!("unsupported deploy lock version {}", lock.version));
    }
    Ok(lock)
}

fn write_lock(path: &Path, lock: &DeployLock) -> Result<(), String> {
    std::fs::write(
        path,
        toml::to_string_pretty(lock).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

fn materialise_kv_id(path: &Path, id: &str) -> bool {
    let Ok(config) = std::fs::read_to_string(path) else {
        return false;
    };
    if !config.contains(KV_NAMESPACE_ID_PLACEHOLDER) {
        return true;
    }
    std::fs::write(path, config.replace(KV_NAMESPACE_ID_PLACEHOLDER, id)).is_ok()
}

/// Fill a generated worker configuration from the committed deploy ledger.
/// This is shared with `bynk dev -- --remote`; local dev leaves placeholders
/// alone because Miniflare does not read the Cloudflare namespace id.
pub fn materialise_deploy_state(
    project_root: &Path,
    worker: &str,
    config: &Path,
) -> Result<bool, String> {
    let text = std::fs::read_to_string(config).map_err(|e| e.to_string())?;
    if !text.contains(KV_NAMESPACE_ID_PLACEHOLDER) {
        return Ok(false);
    }
    let lock = read_lock(&project_root.join(LOCK_FILE))?;
    let Some(id) = lock
        .environments
        .get("default")
        .and_then(|environment| environment.kv.get(worker))
        .map(|namespace| namespace.id.as_str())
    else {
        return Err(format!(
            "remote KV for `{worker}` has not been provisioned; run `bynk deploy` first"
        ));
    };
    if materialise_kv_id(config, id) {
        Ok(true)
    } else {
        Err("could not write generated configuration".into())
    }
}

fn whoami(provenance: &Provenance) -> bool {
    let Some(mut command) = dev::wrangler_command(provenance, "whoami") else {
        return false;
    };
    command
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn create_kv(provenance: &Provenance, name: &str, project_root: &Path) -> Result<String, String> {
    let Some(mut command) = dev::wrangler_command(provenance, "kv") else {
        return Err("wrangler not found".into());
    };
    let output = command
        .arg("namespace")
        .arg("create")
        .arg(name)
        .arg("--json")
        // The generated worker config still carries the placeholder until the
        // namespace exists. Create from the project root so Wrangler cannot
        // load and validate that incomplete config.
        .current_dir(project_root)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    parse_kv_id(&String::from_utf8_lossy(&output.stdout))
        .ok_or_else(|| "wrangler did not return a namespace id".into())
}

fn parse_kv_id(output: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(output)
        .ok()?
        .get("id")?
        .as_str()
        .map(str::to_owned)
}

fn print_plan(plan: &Plan<'_>, format: DeployFormat) {
    match format {
        DeployFormat::Short => {
            if let Some(kv) = &plan.kv {
                println!("kv {} {}", kv.action, kv.namespace);
            }
            println!("deploy {}", plan.worker);
        }
        DeployFormat::Json => println!(
            "{}",
            serde_json::to_string_pretty(plan).expect("plan serialises")
        ),
    }
}

fn derive_plan<'a>(worker: &'a str, needs_kv: bool, recorded: Option<&str>) -> Plan<'a> {
    Plan {
        environment: "default",
        worker,
        kv: needs_kv.then(|| PlanKv {
            action: if recorded.is_some() {
                "reuse"
            } else {
                "create"
            },
            namespace: worker,
        }),
        deploy: true,
    }
}

fn should_refuse_unrecorded_ci(needs_kv: bool, recorded: Option<&str>, ci: bool) -> bool {
    needs_kv && recorded.is_none() && ci
}

fn requires_interactive_confirmation(yes: bool, stdin_is_terminal: bool) -> bool {
    !yes && stdin_is_terminal
}

fn is_ci() -> bool {
    std::env::var_os("CI").is_some_and(|value| value != "false")
}

fn confirm(yes: bool) -> bool {
    if yes {
        return true;
    }
    if !requires_interactive_confirmation(yes, io::stdin().is_terminal()) {
        eprintln!("bynk: refusing to mutate in a non-interactive session without --yes");
        return false;
    }
    eprint!("Deploy to Cloudflare? [y/N] ");
    let _ = io::stderr().flush();
    let mut answer = String::new();
    io::stdin().read_line(&mut answer).is_ok()
        && matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lock_round_trip_is_environment_keyed() {
        let lock = DeployLock {
            version: 1,
            environments: BTreeMap::from([(
                "default".into(),
                Environment {
                    kv: BTreeMap::from([("api".into(), KvNamespace { id: "abc".into() })]),
                },
            )]),
        };
        assert_eq!(
            toml::from_str::<DeployLock>(&toml::to_string_pretty(&lock).unwrap()).unwrap(),
            lock
        );
    }
    #[test]
    fn parses_wrangler_namespace_json() {
        assert_eq!(
            parse_kv_id(r#"{"id":"abc","title":"api"}"#),
            Some("abc".into())
        );
    }
    #[test]
    fn materialises_only_the_placeholder() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("bynk-deploy-{}-{}", std::process::id(), unique));
        std::fs::write(&path, format!("id = \"{KV_NAMESPACE_ID_PLACEHOLDER}\"")).unwrap();
        assert!(materialise_kv_id(&path, "abc"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "id = \"abc\"");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn plan_creates_or_reuses_kv_from_the_ledger() {
        assert_eq!(derive_plan("api", true, None).kv.unwrap().action, "create");
        assert_eq!(
            derive_plan("api", true, Some("namespace-id"))
                .kv
                .unwrap()
                .action,
            "reuse"
        );
        assert!(derive_plan("api", false, None).kv.is_none());
    }

    #[test]
    fn dry_run_and_ci_gates_do_not_reach_mutation() {
        assert!(
            DeployOptions {
                dry_run: true,
                ..Default::default()
            }
            .dry_run
        );
        assert!(should_refuse_unrecorded_ci(true, None, true));
        assert!(!should_refuse_unrecorded_ci(true, Some("id"), true));
        assert!(!should_refuse_unrecorded_ci(true, None, false));
    }

    #[test]
    fn non_interactive_deploy_requires_yes() {
        assert!(!requires_interactive_confirmation(false, false));
        assert!(requires_interactive_confirmation(false, true));
        assert!(!requires_interactive_confirmation(true, false));
    }
}
