//! Slice 3 (ADR 0104) end-to-end proof: a breakpoint set in a `.bynk` handler
//! statement binds and pauses on a worker running under `wrangler dev`'s inspector.
//!
//! This is the productised slice-3 spike. It builds a one-context HTTP worker
//! (Workers target, with source maps), starts `wrangler dev --inspector-port`, and
//! drives a headless CDP session — exactly what a JavaScript debugger does — to
//! confirm the map round-trip *through the wrangler/esbuild bundle* on a real
//! worker. The arg wiring (`--inspect` → `--inspector-port`) is unit-tested in
//! `dev.rs`; this exercises the attach + composed-map breakpoint.
//!
//! Skipped when `wrangler` or `node` is unavailable, so it never blocks CI on a
//! toolchain that lacks the Cloudflare runtime — the always-on guarantee that the
//! emitted handler maps are correct is `bynkc`'s in-process decode goldens.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

fn have(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn tmp() -> PathBuf {
    static C: AtomicU32 = AtomicU32::new(0);
    std::env::temp_dir().join(format!(
        "bynk_devdbg_{}_{}",
        std::process::id(),
        C.fetch_add(1, Ordering::Relaxed)
    ))
}

#[test]
fn workerd_breakpoint_in_bynk_handler_binds_and_pauses() {
    if !have("wrangler") || !have("node") {
        eprintln!("skipping: `wrangler` and/or `node` not available");
        return;
    }

    // A one-context HTTP worker. The handler's effect-let is the breakpoint target.
    let dir = tmp();
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("bynk.toml"), "[project]\nname = \"svc\"\n").unwrap();
    let svc = "context svc\n\nconsumes bynk { Logger }\n\nservice api from http {\n\ton GET(\"/\") () -> Effect[HttpResult[String]] by v: Visitor given Logger {\n\t\tlet _ <- Logger.info(\"hit\")\n\t\tOk(\"ok\")\n\t}\n}\n";
    std::fs::write(dir.join("src").join("svc.bynk"), svc).unwrap();
    let bynk_line = svc.lines().position(|l| l.contains("Logger.info")).unwrap() + 1;

    // Build the Workers output with maps (what `bynk dev` writes to `.bynk/dev/`).
    let build = dir.join("build");
    let opts = bynk_emit::project::CompileOptions::split(
        dir.clone(),
        bynk_emit::project::read_project_paths(&dir),
    )
    .target(bynk_emit::project::BuildTarget::Workers);
    let out = bynk_emit::project::compile_project(&opts)
        .map_err(bynk_emit::project::ProjectFailure::flatten)
        .unwrap_or_else(|e| panic!("compile failed: {e:?}"));
    bynk_emit::write_output(&out, &build).unwrap();
    let worker_dir = build.join("workers").join("svc");
    assert!(
        worker_dir.join("handlers.ts.map").exists(),
        "worker output must carry handlers.ts.map"
    );

    // Unique ports per run (pid-derived) to avoid collisions with a parallel test.
    let base = 9000 + (std::process::id() % 500) as u16 * 2;
    let (inspector_port, app_port) = (base + 1, base);

    let mut wrangler = Command::new("wrangler")
        .args([
            "dev",
            "--inspector-port",
            &inspector_port.to_string(),
            "--port",
            &app_port.to_string(),
        ])
        .current_dir(&worker_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn wrangler dev");

    let result = drive(&worker_dir, inspector_port, app_port, bynk_line);

    let _ = wrangler.kill();
    let _ = wrangler.wait();
    // workerd is a grandchild of wrangler — reap it too (best-effort, unix).
    let _ = Command::new("pkill").arg("-f").arg("workerd").output();
    let _ = std::fs::remove_dir_all(&dir);

    let (status, stdout, stderr) = result;
    assert!(
        status,
        "breakpoint did not bind/pause on the worker\n--- harness stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
    );
    assert!(
        stdout.contains("BIND OK"),
        "harness did not confirm bind\n{stdout}"
    );
}

/// Wait for the inspector, then run the CDP harness; returns `(ok, stdout, stderr)`.
fn drive(
    _worker_dir: &Path,
    inspector_port: u16,
    app_port: u16,
    bynk_line: usize,
) -> (bool, String, String) {
    // Poll the CDP discovery endpoint until the worker's inspector is up.
    let deadline = Instant::now() + Duration::from_secs(30);
    let ready = loop {
        if Instant::now() > deadline {
            break false;
        }
        let probe = Command::new("node")
            .arg("-e")
            .arg(format!(
                "fetch('http://127.0.0.1:{inspector_port}/json').then(r=>r.json()).then(j=>process.exit(j[0]?0:1)).catch(()=>process.exit(1))"
            ))
            .output();
        if probe.map(|o| o.status.success()).unwrap_or(false) {
            break true;
        }
        std::thread::sleep(Duration::from_millis(500));
    };
    if !ready {
        return (
            false,
            String::new(),
            "inspector never became reachable".to_string(),
        );
    }

    let harness = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("support")
        .join("wrangler_attach.mjs");
    let out = Command::new("node")
        .arg(harness)
        .arg(inspector_port.to_string())
        .arg(app_port.to_string())
        .arg(bynk_line.to_string())
        .output()
        .expect("run the CDP harness");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}
