//! Slice 3 (semantic-debugging track, ADR 0105): the debug-metadata sidecar.
//! Compile a project and assert each emitted handler is mapped to its Bynk operation
//! label in `CompiledFile.debug_metadata` (written to disk as `<file>.bynkdbg.json`),
//! keyed by the *emitted* function name so the debugger can relabel a stack frame.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

static N: AtomicU32 = AtomicU32::new(0);

/// A unique temp project dir (per-test, so parallel tests don't race).
fn tmp(tag: &str) -> PathBuf {
    let u = N.fetch_add(1, Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!("bynk_dbgmeta_{}_{u}_{tag}", std::process::id()));
    std::fs::create_dir_all(d.join("src")).unwrap();
    d
}

fn debug_meta(dir: &Path, suffix: &str) -> String {
    let opts = bynkc::CompileOptions::split(dir.to_path_buf(), bynkc::read_project_paths(dir))
        .target(bynkc::BuildTarget::Workers);
    let out = bynkc::compile_project(&opts)
        .map_err(bynkc::ProjectFailure::flatten)
        .unwrap_or_else(|e| panic!("compile failed: {e:?}"));
    out.files
        .iter()
        .find(|f| f.output_path.to_string_lossy().replace('\\', "/").ends_with(suffix))
        .unwrap_or_else(|| panic!("no output file ending {suffix:?}"))
        .debug_metadata
        .clone()
        .unwrap_or_else(|| panic!("{suffix} carries no debug metadata"))
}

#[test]
fn http_handler_label_names_the_operation() {
    let dir = tmp("http");
    std::fs::write(dir.join("bynk.toml"), "[project]\nname = \"svc\"\n").unwrap();
    std::fs::write(
        dir.join("src").join("svc.bynk"),
        "context svc\n\nservice api from http {\n  on GET(\"/\") by v: Visitor () -> Effect[HttpResult[String]] {\n    Ok(\"ok\")\n  }\n}\n",
    )
    .unwrap();
    let meta = debug_meta(&dir, "handlers.ts");
    let _ = std::fs::remove_dir_all(&dir);
    // Keyed by the emitted function name (what the debugger's stack frame carries)…
    assert!(meta.contains("\"http_GET\""), "keyed by the emitted fn name: {meta}");
    // …mapped to the Bynk operation label (method + route).
    assert!(meta.contains("GET \\\"/\\\""), "labelled `GET \"/\"`: {meta}");
}

#[test]
fn agent_method_label_carries_params() {
    let dir = tmp("agent");
    std::fs::write(dir.join("bynk.toml"), "[project]\nname = \"counter\"\n").unwrap();
    std::fs::write(
        dir.join("src").join("counter.bynk"),
        "context counter\n\nagent Counter {\n\tkey id: String\n\n\tstate {\n\t\tn: Int,\n\t}\n\n\ton call bump(amount: Int) -> Effect[Result[Int, String]] {\n\t\tlet next = self.state.n + amount\n\t\tcommit { ...self.state, n: next }\n\t\tOk(next)\n\t}\n}\n",
    )
    .unwrap();
    let meta = debug_meta(&dir, "handlers.ts");
    let _ = std::fs::remove_dir_all(&dir);
    // Agent methods key on the method name and carry their parameter names.
    assert!(meta.contains("\"bump\""), "keyed by the agent method: {meta}");
    assert!(meta.contains("bump(amount)"), "labelled with params: {meta}");
}
