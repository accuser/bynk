//! Run one example on a *real* Workers runtime (#518).
//!
//! The 21 behavioural suites execute compiled output against in-memory
//! fakes; nothing ever ran the emitted Worker on the runtime it targets, so
//! fake-vs-real drift was invisible. This smoke test compiles
//! `examples/hello-world` for the Workers target, strips it to the JS
//! artefact, boots it under `wrangler dev` (embedded workerd — the actual
//! Workers runtime), and asserts an end-to-end HTTP round-trip.
//!
//! Like the tsc-verification stage, this skips loudly when the toolchain is
//! unavailable; `BYNK_REQUIRE_WORKERD=1` turns the skip into a failure (CI).

use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const REQUIRE_ENV: &str = "BYNK_REQUIRE_WORKERD";

/// Pinned provisioning, per the repo's npx convention.
const WRANGLER: &str = "wrangler@4";

fn tool_exists(name: &str) -> bool {
    which::which(name).is_ok()
}

fn skip(reason: &str) -> bool {
    eprintln!("\n!!! WORKERS-RUNTIME SMOKE SKIPPED !!!\n{reason}\n");
    if std::env::var(REQUIRE_ENV).is_ok() {
        panic!("{REQUIRE_ENV} is set but {reason}");
    }
    true
}

/// Kill the `wrangler dev` child on every exit path — a leaked workerd holds
/// the port and outlives the test binary.
struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn hello_world_serves_on_workerd() {
    if !tool_exists("npx") && skip("`npx` is not on PATH") {
        return;
    }
    if !tool_exists("node") && skip("`node` is not on PATH") {
        return;
    }

    // Compile the example for Workers and strip to the JS artefact — the
    // form `wrangler dev` runs directly, no tsc in the loop (ADR 0137).
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/hello-world");
    let paths = bynkc::read_project_paths(&root);
    let out = bynkc::compile_project(
        &bynkc::CompileOptions::split(root, paths).target(bynkc::BuildTarget::Workers),
    )
    .map_err(bynkc::ProjectFailure::flatten)
    .expect("hello-world compiles for Workers");
    let out = bynkc::strip_project_to_js(out).expect("hello-world strips to JS");

    let tmp = std::env::temp_dir().join(format!("bynk-workerd-smoke-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();
    bynkc::write_output(&out, &tmp).unwrap();

    let worker_dir = tmp.join("workers/hello-web");
    assert!(
        worker_dir.join("index.js").is_file() && worker_dir.join("wrangler.toml").is_file(),
        "example layout changed — update this test's worker path"
    );

    // A pid-derived port keeps parallel test binaries off each other.
    let port = 20000 + (std::process::id() % 10000) as u16;
    let child = Command::new("npx")
        .args(["-y", WRANGLER, "dev", "--port", &port.to_string()])
        .current_dir(&worker_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let child = match child {
        Ok(c) => KillOnDrop(c),
        Err(e) => {
            if skip(&format!("could not launch npx: {e}")) {
                return;
            }
            unreachable!()
        }
    };

    // First run provisions wrangler + workerd via npx; allow a generous
    // boot window, then a strict assertion.
    let deadline = Instant::now() + Duration::from_secs(180);
    let url = format!("http://127.0.0.1:{port}/");
    let mut last_err = String::new();
    loop {
        if Instant::now() > deadline {
            let mut child = child;
            let _ = child.0.kill();
            let mut logs = String::new();
            if let Some(mut e) = child.0.stderr.take() {
                let _ = e.read_to_string(&mut logs);
            }
            if skip(&format!(
                "wrangler dev did not serve within the boot window (likely no \
                 network to provision {WRANGLER}); last error: {last_err}\n{logs}"
            )) {
                return;
            }
            unreachable!()
        }
        std::thread::sleep(Duration::from_millis(500));
        match fetch(&url) {
            Ok(body) => {
                assert!(
                    body.contains("Hello, World!"),
                    "unexpected body from workerd: {body}"
                );
                // Drop kills wrangler (and its workerd) before cleanup.
                drop(child);
                let _ = fs::remove_dir_all(&tmp);
                return;
            }
            Err(e) => last_err = e,
        }
    }
}

/// A dependency-free HTTP GET (the test crate has no HTTP client): one
/// request, HTTP/1.1, connection-close.
fn fetch(url: &str) -> Result<String, String> {
    use std::io::Write;
    let addr = url
        .strip_prefix("http://")
        .and_then(|r| r.split('/').next())
        .ok_or("bad url")?;
    let mut stream = std::net::TcpStream::connect_timeout(
        &addr.parse().map_err(|e| format!("{e}"))?,
        Duration::from_secs(2),
    )
    .map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    write!(
        stream,
        "GET / HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    )
    .map_err(|e| e.to_string())?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|e| e.to_string())?;
    if !response.starts_with("HTTP/1.1 200") {
        return Err(format!(
            "non-200: {}",
            response.lines().next().unwrap_or("<empty>")
        ));
    }
    Ok(response)
}
