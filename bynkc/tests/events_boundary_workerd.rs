//! Events track (spine #936), issue #973 — the receiving `/_bynk/event/`
//! route on Cloudflare Workers actually rejects a malformed payload/envelope
//! at runtime, on a real `workerd`.
//!
//! Prior to this fix, the whole delivery chain (mint → fan-out DO →
//! `deliverEvent` → this route → the subscriber's `compose.ts` wrapper)
//! carried the payload and envelope as `unknown`, cast `as any` at the final
//! hop — no `deserialise_*` was ever called. `events_workers_wiring.rs`
//! proves the fix's *generated code shape* (string assertions + `tsc
//! --strict`); this file is the genuinely discriminating proof: a live
//! request against a real running Worker, showing a malformed body is
//! rejected with `400` **and never reaches the handler** — the half a
//! string/type check alone cannot show.
//!
//! Uses a context that subscribes to its own declared event
//! (`from Events(Tick)` on the same event `Tick` it declares) so the whole
//! test needs only **one** `wrangler dev` process, not the two-context
//! publisher/subscriber pair `events_ordering_workerd.rs` needs to measure
//! cross-context ordering — this test POSTs directly to the subscriber's
//! `/_bynk/event/<service>` route itself, bypassing emission and fan-out
//! entirely, so no publisher is needed. Confirmed self-subscription compiles
//! and reaches the same generated route/codec shape as the cross-context
//! case (`emit_consumed_context_helpers`'s widening in #973 only matters when
//! the event's *owner* is a different, consumed context; a locally-declared
//! event already had its codec via the pre-existing `local_boundary` path).
//!
//! Same toolchain-skip, `BYNK_REQUIRE_WORKERD` gate, and SIGTERM-then-reap
//! teardown idiom as `events_ordering_workerd.rs` — see that file's module
//! doc for why each of those exists; not restated here.
//!
//! Events slice 3a (#972) adds a second test,
//! `events_boundary_field_default_cross_context_on_workerd`, that does need a
//! genuine two-context project. A field default lowers to its *wire* form
//! specifically so a subscriber's own regenerated codec never needs a
//! qualified value-level reference into the publisher's module — the one
//! failure mode a self-subscription fixture structurally cannot show, since
//! even a wrongly-qualified reference resolves fine when publisher and
//! subscriber are the same module. That test still starts only **one**
//! `wrangler dev` (the subscriber's), POSTing straight to its
//! `/_bynk/event/<service>` route exactly as above — the publisher context
//! is compiled (so its worker directory exists) but never run, since nothing
//! here exercises emission or fan-out.

use bynkc::{BuildTarget, CompileOptions};
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

mod require;

const REQUIRE_ENV: &str = "BYNK_REQUIRE_WORKERD";
const WRANGLER: &str = "wrangler@4";

const SOURCE: &str = r#"context boundary.probe

exports transparent { Tick }

event Tick = {
  tag: String,
}

agent Trace {
  key id: String
  store seen: Cell[String] = ""

  on call note(tag: String) -> Effect[()] {
    let _ <- seen.update((s) => s.concat(tag))
    Effect.pure(())
  }

  on call read() -> Effect[String] {
    Effect.pure(seen)
  }
}

service OnTick from Events(Tick) {
  on event(e: Tick) -> Effect[()] {
    let _ <- Trace("main").note(e.tag)
    Effect.pure(())
  }
}

service traceApi from http {
  on GET("/trace") () -> Effect[HttpResult[String]] by v: Visitor {
    let s <- Trace("main").read()
    Ok(s)
  }
}
"#;

/// #972: `commerce.order` declares an event with one defaulted field
/// (`region`) and one non-defaulted field (`orderId`) — the non-defaulted
/// field stays the negative control proving a missing key with *no* declared
/// default still fails structural-mismatch, exactly as it did before this
/// slice.
const ORDER_SOURCE: &str = r#"context commerce.order

exports transparent { PaymentConfirmed }

event PaymentConfirmed = {
  orderId: String,
  region: String = "domestic",
}
"#;

/// #972: `commerce.notifications` only *consumes* `commerce.order` and
/// subscribes to its event — it never calls a method on it, so this also
/// exercises the `emit_consumed_context_helpers` "event roots" widening
/// (#973) that makes the payload's codec reachable here at all.
const NOTIFICATIONS_SOURCE: &str = r#"context commerce.notifications

consumes commerce.order

agent Trace {
  key id: String
  store seen: Cell[String] = ""

  on call note(v: String) -> Effect[()] {
    let _ <- seen.update((s) => s.concat(v))
    Effect.pure(())
  }

  on call read() -> Effect[String] {
    Effect.pure(seen)
  }
}

service OnPayment from Events(PaymentConfirmed) {
  on event(e: PaymentConfirmed) -> Effect[()] {
    let _ <- Trace("main").note(e.region)
    Effect.pure(())
  }
}

service traceApi from http {
  on GET("/trace") () -> Effect[HttpResult[String]] by v: Visitor {
    let s <- Trace("main").read()
    Ok(s)
  }
}
"#;

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
    eprintln!("\n!!! EVENTS BOUNDARY (WORKERD) SKIPPED !!!\n{reason}\n");
    if required() {
        panic!("{REQUIRE_ENV} is set but {reason}");
    }
    true
}

/// Mirrors `events_ordering_workerd.rs`'s `request_stop` — SIGTERM, not
/// `Child::kill`'s SIGKILL, so wrangler tears down its own `node`/`workerd`
/// children instead of stranding an orphan still holding the port.
fn request_stop(child: &mut Child) {
    #[cfg(unix)]
    {
        let sent = Command::new("kill")
            .arg("-TERM")
            .arg(child.id().to_string())
            .status()
            .is_ok_and(|s| s.success());
        if sent {
            return;
        }
    }
    let _ = child.kill();
}

fn reap(child: &mut Child) {
    const GRACE: Duration = Duration::from_secs(10);
    const TICK: Duration = Duration::from_millis(50);
    let mut waited = Duration::ZERO;
    while waited < GRACE {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => {}
            Err(_) => break,
        }
        std::thread::sleep(TICK);
        waited += TICK;
    }
    let _ = child.kill();
    let _ = child.wait();
}

struct Wrangler(Option<Child>);

impl Drop for Wrangler {
    fn drop(&mut self) {
        if let Some(c) = &mut self.0 {
            request_stop(c);
            reap(c);
        }
    }
}

/// A raw HTTP/1.1 client returning `(status, body)` — `events_ordering_workerd.rs`'s
/// `http()` only returns the body and treats non-200 as an error, which loses
/// exactly the status code this test needs to assert on.
fn http_status(
    method: &str,
    port: u16,
    path: &str,
    body: Option<&str>,
) -> Result<(u16, String), String> {
    let addr = format!("127.0.0.1:{port}");
    let mut stream = std::net::TcpStream::connect_timeout(
        &addr.parse().map_err(|e| format!("{e}"))?,
        Duration::from_secs(2),
    )
    .map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .map_err(|e| e.to_string())?;
    let mut request = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n");
    if let Some(b) = body {
        request.push_str("Content-Type: application/json\r\n");
        request.push_str(&format!("Content-Length: {}\r\n", b.len()));
        request.push_str("\r\n");
        request.push_str(b);
    } else {
        request.push_str("\r\n");
    }
    stream
        .write_all(request.as_bytes())
        .map_err(|e| e.to_string())?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|e| e.to_string())?;
    let status_line = response.lines().next().unwrap_or("");
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| format!("unparseable status line: {status_line:?}"))?;
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();
    Ok((status, body))
}

fn unquote_json_string(s: &str) -> String {
    s.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(s)
        .to_string()
}

fn read_log(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| format!("<could not read {path:?}: {e}>"))
}

fn valid_envelope() -> &'static str {
    r#""envelope":{"eventId":"e1","publisherId":"boundary.probe","emittedAt":1700000000000,"schemaVersion":1}"#
}

#[test]
fn events_boundary_rejects_malformed_payload_and_envelope_on_workerd() {
    if !tool_exists("npx") && skip("`npx` is not on PATH") {
        return;
    }
    if !tool_exists("node") && skip("`node` is not on PATH") {
        return;
    }

    let tmp = std::env::temp_dir().join(format!("bynk-events-boundary-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    let proj = tmp.join("proj/boundary");
    fs::create_dir_all(&proj).unwrap();
    fs::write(proj.join("probe.bynk"), SOURCE).unwrap();

    let out = match bynkc::compile_project(
        &CompileOptions::single(tmp.join("proj")).target(BuildTarget::Workers),
    ) {
        Ok(o) => o,
        Err(failure) => panic!(
            "compile the boundary-validation project to workers:\n{}",
            bynkc::render_project_errors(&failure.flatten())
        ),
    };
    let out = bynkc::strip_project_to_js(out).expect("boundary project strips to JS");
    bynkc::write_output(&out, &tmp).unwrap();

    let dir = tmp.join("workers/boundary-probe");
    assert!(
        dir.join("index.js").is_file() && dir.join("wrangler.toml").is_file(),
        "expected workers/boundary-probe — context dasherisation changed?"
    );

    // Offset from both `workers_runtime_smoke.rs`'s and
    // `events_ordering_workerd.rs`'s port bands so three tests spawning
    // `wrangler dev` concurrently cannot collide.
    let base = 40_000 + 2 * (std::process::id() % 4_000) as u16;
    let port = base;
    let inspector_port = base + 1;

    let log_path = tmp.join("wrangler.log");
    let out_log = fs::File::create(&log_path).unwrap();
    let err_log = out_log.try_clone().unwrap();
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
        .current_dir(&dir)
        .stdout(Stdio::from(out_log))
        .stderr(Stdio::from(err_log))
        .spawn();
    let child = match child {
        Ok(c) => c,
        Err(_) => {
            if skip("could not launch npx") {
                return;
            }
            unreachable!()
        }
    };
    let wrangler = Wrangler(Some(child));

    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        if Instant::now() > deadline {
            let log = read_log(&log_path);
            drop(wrangler);
            let _ = fs::remove_dir_all(&tmp);
            if skip(&format!(
                "wrangler dev did not serve within the boot window (likely no \
                 network to provision {WRANGLER})\n--- wrangler log ---\n{log}"
            )) {
                return;
            }
            unreachable!()
        }
        if http_status("GET", port, "/trace", None).is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }

    let read_trace = || -> String {
        let (status, body) = http_status("GET", port, "/trace", None).expect("read /trace");
        assert_eq!(status, 200, "reading /trace itself must succeed");
        unquote_json_string(&body)
    };

    // Baseline: nothing has been delivered yet.
    assert_eq!(read_trace(), "", "trace must start empty");

    // (1) A malformed payload (wrong field type) must be rejected with 400,
    // and — the half a generated-code assertion alone cannot show — the
    // handler must never have run.
    let bad_payload_body = format!(r#"{{"payload":{{"tag":123}},{}}}"#, valid_envelope());
    let (status, resp_body) =
        http_status("POST", port, "/_bynk/event/OnTick", Some(&bad_payload_body))
            .expect("POST malformed payload");
    assert_eq!(
        status, 400,
        "a malformed event payload must be rejected with 400, got body: {resp_body}"
    );
    assert_eq!(
        read_trace(),
        "",
        "a rejected payload must never reach the handler"
    );

    // (2) A malformed envelope (non-string publisherId) must also be
    // rejected with 400, and likewise never reach the handler.
    let bad_envelope_body = r#"{"payload":{"tag":"x"},"envelope":{"eventId":"e1","publisherId":7,"emittedAt":1700000000000,"schemaVersion":1}}"#;
    let (status, resp_body) =
        http_status("POST", port, "/_bynk/event/OnTick", Some(bad_envelope_body))
            .expect("POST malformed envelope");
    assert_eq!(
        status, 400,
        "a malformed envelope must be rejected with 400, got body: {resp_body}"
    );
    assert_eq!(
        read_trace(),
        "",
        "a rejected envelope must never reach the handler"
    );

    // (3) The positive control: a well-formed payload and envelope must
    // still be accepted (204) and actually reach the handler — without this,
    // (1) and (2) could pass merely because the route rejects everything.
    let good_body = format!(r#"{{"payload":{{"tag":"ok"}},{}}}"#, valid_envelope());
    let (status, resp_body) = http_status("POST", port, "/_bynk/event/OnTick", Some(&good_body))
        .expect("POST valid event");
    assert_eq!(
        status, 204,
        "a well-formed event must be accepted, got body: {resp_body}"
    );
    assert_eq!(
        read_trace(),
        "ok",
        "a well-formed event must actually reach and run the handler"
    );

    drop(wrangler);
    let _ = fs::remove_dir_all(&tmp);
}

/// #972 (Events slice 3a): a subscriber's *own* regenerated codec for a
/// consumed event's defaulted field actually falls back to the default on a
/// real `workerd`, and a missing non-defaulted field still fails
/// structural-mismatch. See the module doc for why this needs a genuine
/// two-context project rather than self-subscription.
#[test]
fn events_boundary_field_default_cross_context_on_workerd() {
    if !tool_exists("npx") && skip("`npx` is not on PATH") {
        return;
    }
    if !tool_exists("node") && skip("`node` is not on PATH") {
        return;
    }

    let tmp = std::env::temp_dir().join(format!(
        "bynk-events-boundary-default-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&tmp);
    let proj = tmp.join("proj");
    fs::create_dir_all(proj.join("commerce")).unwrap();
    fs::write(proj.join("commerce/order.bynk"), ORDER_SOURCE).unwrap();
    fs::write(
        proj.join("commerce/notifications.bynk"),
        NOTIFICATIONS_SOURCE,
    )
    .unwrap();

    let out = match bynkc::compile_project(
        &CompileOptions::single(proj.clone()).target(BuildTarget::Workers),
    ) {
        Ok(o) => o,
        Err(failure) => panic!(
            "compile the cross-context field-default project to workers:\n{}",
            bynkc::render_project_errors(&failure.flatten())
        ),
    };
    let out = bynkc::strip_project_to_js(out).expect("field-default project strips to JS");
    bynkc::write_output(&out, &tmp).unwrap();

    let dir = tmp.join("workers/commerce-notifications");
    assert!(
        dir.join("index.js").is_file() && dir.join("wrangler.toml").is_file(),
        "expected workers/commerce-notifications — context dasherisation changed?"
    );
    // The publisher's own worker directory must exist (the project compiled
    // it), even though this test never starts it.
    assert!(
        tmp.join("workers/commerce-order/index.js").is_file(),
        "expected workers/commerce-order to also be emitted"
    );

    // A distinct band from both `events_ordering_workerd.rs`'s
    // (30_000 + 4*pid%4000) and this file's own self-subscription test's
    // (40_000 + 2*pid%4000), so all three can run concurrently without a
    // port collision.
    let base = 55_000 + 2 * (std::process::id() % 4_000) as u16;
    let port = base;
    let inspector_port = base + 1;

    let log_path = tmp.join("wrangler.log");
    let out_log = fs::File::create(&log_path).unwrap();
    let err_log = out_log.try_clone().unwrap();
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
        .current_dir(&dir)
        .stdout(Stdio::from(out_log))
        .stderr(Stdio::from(err_log))
        .spawn();
    let child = match child {
        Ok(c) => c,
        Err(_) => {
            if skip("could not launch npx") {
                return;
            }
            unreachable!()
        }
    };
    let wrangler = Wrangler(Some(child));

    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        if Instant::now() > deadline {
            let log = read_log(&log_path);
            drop(wrangler);
            let _ = fs::remove_dir_all(&tmp);
            if skip(&format!(
                "wrangler dev did not serve within the boot window (likely no \
                 network to provision {WRANGLER})\n--- wrangler log ---\n{log}"
            )) {
                return;
            }
            unreachable!()
        }
        if http_status("GET", port, "/trace", None).is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }

    let read_trace = || -> String {
        let (status, body) = http_status("GET", port, "/trace", None).expect("read /trace");
        assert_eq!(status, 200, "reading /trace itself must succeed");
        unquote_json_string(&body)
    };

    assert_eq!(read_trace(), "", "trace must start empty");

    // (1) The wire event's `region` key is entirely absent — the subscriber's
    // own regenerated `deserialise_PaymentConfirmed` must fall back to the
    // declared default (`"domestic"`) rather than fail structural-mismatch.
    let missing_default_body = format!(r#"{{"payload":{{"orderId":"o1"}},{}}}"#, valid_envelope());
    let (status, resp_body) = http_status(
        "POST",
        port,
        "/_bynk/event/OnPayment",
        Some(&missing_default_body),
    )
    .expect("POST payload missing the defaulted field");
    assert_eq!(
        status, 204,
        "a payload missing only a defaulted field must be accepted, got body: {resp_body}"
    );
    assert_eq!(
        read_trace(),
        "domestic",
        "the handler must observe the declared default for the missing field"
    );

    // (2) The wire event's `orderId` key (no default) is missing — this must
    // still fail structural-mismatch exactly as before this slice, and must
    // never reach the handler (trace stays at its step-(1) value, not
    // appended to).
    let missing_required_body = format!(
        r#"{{"payload":{{"region":"international"}},{}}}"#,
        valid_envelope()
    );
    let (status, resp_body) = http_status(
        "POST",
        port,
        "/_bynk/event/OnPayment",
        Some(&missing_required_body),
    )
    .expect("POST payload missing the non-defaulted field");
    assert_eq!(
        status, 400,
        "a payload missing a non-defaulted field must still be rejected with 400, got body: {resp_body}"
    );
    assert_eq!(
        read_trace(),
        "domestic",
        "a rejected payload must never reach the handler"
    );

    drop(wrangler);
    let _ = fs::remove_dir_all(&tmp);
}
