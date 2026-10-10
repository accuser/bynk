//! #1831: every name an emitted module uses is bound in that module.
//!
//! Runs [`bynk_ts::unbound_names`] over every TypeScript module the positive
//! corpus emits. That catches the `TS2304`/`TS2552` family (#1736, #1778,
//! #1815, #1818, #1823, #1829), where the emitter names a type or calls a codec
//! the module never imports, without `tsc` on `PATH`.
//!
//! **Both targets.** A project fixture is compiled for bundle *and* Workers,
//! whatever its `target.txt` says. A fixture built only for bundle never had
//! its Workers output type-checked, and that is where #1845 and #1846 were
//! found. `bynkc test`'s layout is the bundle output with the Workers output
//! overlaid, so its modules are covered too. A single-file fixture is compiled
//! in memory through the project pipeline (its own path has no tree to walk).
//!
//! **Combinations no fixture has.** [`repros`] holds minimal programs for open
//! defects whose shape the corpus lacks.
//!
//! **Known defects** are listed in [`KNOWN`] with their issue, strictly: an
//! unlisted report fails, and so does a listed one that no longer reports.
//! The fix for an issue deletes its entry, the same discipline as
//! `behaviour_fixtures.rs`'s `fail` lines. The check cannot see into opaque
//! text (`bynk_ts::unbound_names`' module doc), so a clean run here does not
//! replace `tsc_verify`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use bynkc::BuildTarget;

/// `(source, target, module, unbound names, issue)`. `source` is a fixture
/// directory name or a [`repros`] name.
const KNOWN: &[(&str, &str, &str, &[&str], u32)] = &[];

/// Minimal projects for open defects no positive fixture exhibits:
/// `(name, [(path, source)])`, compiled for bundle.
fn repros() -> Vec<(&'static str, Vec<(&'static str, &'static str)>)> {
    // #1829's repro became `1829_agent_state_unread_commons_field`.
    Vec::new()
}

fn target_name(target: BuildTarget) -> &'static str {
    match target {
        BuildTarget::Workers => "workers",
        _ => "bundle",
    }
}

/// A module's output path, `/`-separated on every platform.
fn module_key(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

type Reports = BTreeMap<(String, &'static str, String), Vec<String>>;

/// Record every module of `out` with an unbound name.
fn record(
    reports: &mut Reports,
    source: &str,
    target: BuildTarget,
    out: &bynkc::ProjectOutput,
) -> usize {
    let mut modules = 0;
    for (path, doc) in &out.artefacts.docs {
        if let bynkc::Document::Ts(program) = doc {
            modules += 1;
            let unbound = bynk_ts::unbound_names(program);
            if !unbound.is_empty() {
                reports.insert(
                    (source.to_string(), target_name(target), module_key(path)),
                    unbound,
                );
            }
        }
    }
    modules
}

fn compile_project_fixture(dir: &Path, target: BuildTarget) -> bynkc::ProjectOutput {
    let options = if dir.join("bynk.toml").exists() {
        let paths = bynkc::try_read_project_paths(dir).expect("well-formed fixture manifest");
        bynk_testkit::compile_options_split(dir.to_path_buf(), paths)
    } else {
        bynk_testkit::compile_options_single(dir.join("src"))
    };
    bynkc::compile_project(&options.target(target)).unwrap_or_else(|f| {
        panic!(
            "{} must compile for {}:\n{}",
            dir.display(),
            target_name(target),
            bynkc::render_project_errors(&f.flatten())
        )
    })
}

#[test]
fn every_emitted_module_binds_every_name_it_uses() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/positive");
    let mut dirs: Vec<PathBuf> = fs::read_dir(&root)
        .expect("positive fixtures")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();

    let mut reports = Reports::new();
    let mut modules = 0;
    for dir in &dirs {
        let name = dir.file_name().unwrap().to_string_lossy().to_string();
        if dir.join("src").is_dir() {
            for target in [BuildTarget::Bundle, BuildTarget::Workers] {
                let out = compile_project_fixture(dir, target);
                modules += record(&mut reports, &name, target, &out);
            }
        } else {
            let source = fs::read_to_string(dir.join("input.bynk")).expect("input.bynk");
            let out = bynk_emit::project::compile_in_memory(
                &source,
                BuildTarget::Bundle,
                Default::default(),
            )
            .unwrap_or_else(|_| panic!("{name} must compile"));
            modules += record(&mut reports, &name, BuildTarget::Bundle, &out);
        }
    }

    let scratch = std::env::temp_dir().join(format!("bynk-import-closure-{}", std::process::id()));
    for (name, files) in repros() {
        let dir = scratch.join(name);
        let _ = fs::remove_dir_all(&dir);
        for (path, src) in &files {
            let p = dir.join(path);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, src).unwrap();
        }
        let out = compile_project_fixture(&dir, BuildTarget::Bundle);
        modules += record(&mut reports, name, BuildTarget::Bundle, &out);
    }
    let _ = fs::remove_dir_all(&scratch);

    assert!(modules > 1000, "only {modules} modules checked");

    let known: Reports = KNOWN
        .iter()
        .map(|(source, target, module, names, _)| {
            (
                (source.to_string(), *target, module.to_string()),
                names.iter().map(|n| n.to_string()).collect(),
            )
        })
        .collect();
    let unexpected: Vec<String> = reports
        .iter()
        .filter(|(k, v)| known.get(*k) != Some(*v))
        .map(|((s, t, m), v)| format!("  {s} [{t}] {m}: {v:?}"))
        .collect();
    let fixed: Vec<String> = KNOWN
        .iter()
        .filter(|(s, t, m, ..)| !reports.contains_key(&(s.to_string(), *t, m.to_string())))
        .map(|(s, t, m, _, issue)| format!("  {s} [{t}] {m} (#{issue})"))
        .collect();
    assert!(
        unexpected.is_empty() && fixed.is_empty(),
        "{modules} modules checked.\n\
         Modules using a name they never bind (each is a TS2304/TS2552 tsc would report):\n{}\n\
         Known defects that no longer report (delete their `KNOWN` entries):\n{}",
        unexpected.join("\n"),
        fixed.join("\n"),
    );
}
