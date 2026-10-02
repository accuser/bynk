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
//! unavailable; a *non-empty* `BYNK_REQUIRE_WORKERD` (CI sets `1`) turns the
//! skip into a failure. Empty counts as unset — see `required`.

use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

mod require;
mod wrangler;

const REQUIRE_ENV: &str = "BYNK_REQUIRE_WORKERD";

/// Pinned provisioning, per the repo's npx convention — the spec itself lives in
/// [`wrangler`], shared with the other smokes and with CI's pre-warm step.
const WRANGLER: &str = wrangler::SPEC;

fn tool_exists(name: &str) -> bool {
    which::which(name).is_ok()
}

/// Route through `cmd /C` on Windows so npm's `npx.cmd` shim resolves
/// (Rust's CreateProcess refuses batch scripts directly — BatBadBut).
fn base_command(program: &str) -> Command {
    if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(program);
        c
    } else {
        Command::new(program)
    }
}

/// Is this run required to actually reach workerd?
///
/// Presence alone is not enough — empty counts as unset. The contract, and the
/// history that made it necessary, live in [`require`].
fn required() -> bool {
    require::is_required(REQUIRE_ENV)
}

fn skip(reason: &str) -> bool {
    eprintln!("\n!!! WORKERS-RUNTIME SMOKE SKIPPED !!!\n{reason}\n");
    if required() {
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
    let paths = bynkc::try_read_project_paths(&root).expect("well-formed fixture manifest");
    let out = bynkc::compile_project(
        &bynk_testkit::compile_options_split(root, paths).target(bynkc::BuildTarget::Workers),
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
    let child = base_command("npx")
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
        match fetch(&url, "/") {
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

/// #1649 (runtime-semantics track §3.2): agent state round-trips through real
/// Durable Object storage. The behavioural fixtures run the bundle target, whose
/// storage is an in-memory stand-in. Only workerd's own storage (V8
/// serialisation, a fresh copy on every `get`) proves the wire-shape encode and
/// decode end to end. One request writes an enum and an `Option`; a later
/// request, which reloads state from storage, reads both back. Before #1649 that
/// read faulted with `RehydrationViolation`. #1685: the same requests write a
/// store `Map` entry under the key `__proto__`, which must come back as an own
/// entry through workerd's V8 serialisation.
const AGENT_STATE_SOURCE: &str = r#"context smoke

type Light = enum { Red, Green }

agent Lamp {
  key id: String
  store light: Cell[Light] = Red
  store reading: Cell[Option[Int]]
  store tags: Map[String, Int]

  on call turnGreen() -> Effect[()] { light := Green }
  on call isGreen() -> Effect[Bool] { light == Green }
  on call note(n: Int) -> Effect[()] { reading := Some(n) }
  on call tag(k: String) -> Effect[()] {
    let _ <- tags.put(k, 1)
    ()
  }
  on call tagCount() -> Effect[Int] {
    let n <- tags.size()
    n
  }
  on call hasTag(k: String) -> Effect[Bool] {
    let r <- tags.contains(k)
    r
  }
  on call lastReading() -> Effect[Int] {
    match reading {
      Some(n) => n
      None => 0
    }
  }
}

service api from http {
  on GET("/") () -> Effect[HttpResult[String]] by v: Visitor {
    Ok("up")
  }

  on GET("/set") () -> Effect[HttpResult[String]] by v: Visitor {
    do Lamp("k").turnGreen()
    do Lamp("k").note(7)
    do Lamp("k").tag("__proto__")
    Ok("set")
  }

  on GET("/get") () -> Effect[HttpResult[String]] by v: Visitor {
    let green <- Lamp("k").isGreen()
    let n <- Lamp("k").lastReading()
    let t <- Lamp("k").tagCount()
    let p <- Lamp("k").hasTag("__proto__")
    let c <- Lamp("k").hasTag("constructor")
    Ok("green=\(green) reading=\(n) tags=\(t) proto=\(p) ctor=\(c)")
  }
}
"#;

#[test]
fn agent_state_round_trips_on_workerd() {
    if !tool_exists("npx") && skip("`npx` is not on PATH") {
        return;
    }
    if !tool_exists("node") && skip("`node` is not on PATH") {
        return;
    }

    let tmp = std::env::temp_dir().join(format!("bynk-workerd-state-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    let src = tmp.join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("smoke.bynk"), AGENT_STATE_SOURCE).unwrap();
    let out = bynkc::compile_project(
        &bynk_testkit::compile_options_single(src).target(bynkc::BuildTarget::Workers),
    )
    .map_err(bynkc::ProjectFailure::flatten)
    .expect("the agent-state smoke compiles for Workers");
    let out = bynkc::strip_project_to_js(out).expect("the agent-state smoke strips to JS");
    let out_dir = tmp.join("out");
    bynkc::write_output(&out, &out_dir).unwrap();
    let worker_dir = out_dir.join("workers/smoke");
    assert!(
        worker_dir.join("index.js").is_file() && worker_dir.join("wrangler.toml").is_file(),
        "smoke layout changed — update this test's worker path"
    );

    // Offset from `hello_world_serves_on_workerd`'s port, with its own
    // inspector port (wrangler's default 9229 is shared otherwise), so the two
    // tests in this binary can run concurrently, as the events smokes do.
    let port = 30000 + (std::process::id() % 10000) as u16;
    let inspector_port = port + 1;
    let child = base_command("npx")
        .args([
            "-y",
            WRANGLER,
            "dev",
            "--port",
            &port.to_string(),
            "--inspector-port",
            &inspector_port.to_string(),
        ])
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
        match fetch(&url, "/") {
            Ok(_) => break,
            Err(e) => last_err = e,
        }
    }

    let before = fetch(&url, "/get").expect("GET /get before any write");
    assert!(
        before.contains("green=false reading=0 tags=0 proto=false ctor=false"),
        "a fresh key reads its zero values: {before}"
    );
    let set = fetch(&url, "/set").expect("GET /set writes the agent's state");
    assert!(set.contains("set"), "unexpected /set body: {set}");
    let after = fetch(&url, "/get").expect("GET /get reloads the written state");
    assert!(
        after.contains("green=true reading=7 tags=1 proto=true ctor=false"),
        "the enum, Option and `__proto__` map entry written by /set must read back \
         after a reload: {after}"
    );

    drop(child);
    let _ = fs::remove_dir_all(&tmp);
}

/// A dependency-free HTTP GET of `path` (the test crate has no HTTP client):
/// one request, HTTP/1.1, connection-close.
fn fetch(url: &str, path: &str) -> Result<String, String> {
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
    // A route that reaches a Durable Object instantiates it on first use,
    // which takes several seconds on a cold Windows runner; 20s matches the
    // events workerd smokes.
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .map_err(|e| e.to_string())?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
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
