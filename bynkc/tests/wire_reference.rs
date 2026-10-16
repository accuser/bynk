//! #1969: the cross-context wire format, version 1, is a published contract
//! (`site/src/content/docs/book/reference/wire-format.md`). These guards keep
//! the page, the emitter and the runtime in agreement:
//!
//! - the committed page is exactly what `bynk_check::wire_format::reference`
//!   renders;
//! - the hashes the page shows for its example context are the constants the
//!   emitter stamps for it;
//! - the emitted `/_bynk/call/` dispatch makes the published checks in the
//!   published order;
//! - the runtime's `callService` sends the published method, path and headers;
//! - every example on the page, run through the codecs the compiler generates
//!   for the example context on Node, decodes and encodes as the page says, and
//!   the emitted dispatch answers each published failure with its published
//!   status and body.
//!
//! Regenerate the page after an intended change with:
//!     BYNK_BLESS=1 cargo test -p bynkc --test wire_reference
//! The Node guard skips loudly without `tsc` and `node`; `BYNK_REQUIRE_TSC=1`
//! turns the skip into a failure.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use bynk_check::wire_format::reference::{
    EXAMPLE_SOURCE, EXAMPLES, Outcome, example_contracts, render_reference,
};
use bynk_check::wire_format::{self as wf, CallCheck};
use bynkc::BuildTarget;

const REQUIRE_ENV: &str = "BYNK_REQUIRE_TSC";

fn repo(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(rel)
}

#[test]
fn generated_wire_format_page_is_up_to_date() {
    let page = repo("site/src/content/docs/book/reference/wire-format.md");
    let rendered = common::to_site_page(&render_reference(), "reference");

    if std::env::var_os("BYNK_BLESS").is_some() {
        fs::write(&page, &rendered).unwrap();
        return;
    }

    let current = fs::read_to_string(&page).unwrap_or_default();
    assert_eq!(
        current, rendered,
        "site/src/content/docs/book/reference/wire-format.md is out of date with the wire format.\n\
         Regenerate with: BYNK_BLESS=1 cargo test -p bynkc --test wire_reference"
    );
}

/// Compile [`EXAMPLE_SOURCE`] to Workers and write the output under `dir`.
fn compile_example(dir: &Path) {
    let _ = fs::remove_dir_all(dir);
    let proj = dir.join("proj/wire");
    fs::create_dir_all(&proj).unwrap();
    fs::write(proj.join("sample.bynk"), EXAMPLE_SOURCE).unwrap();
    let out = match bynkc::compile_project(
        &bynk_testkit::compile_options_single(dir.join("proj")).target(BuildTarget::Workers),
    ) {
        Ok(o) => o,
        Err(failure) => panic!(
            "compile the wire-format example to Workers:\n{}",
            bynkc::render_project_errors(&failure.flatten())
        ),
    };
    let run_dir = dir.join("run");
    for (path, doc) in &out.artefacts.docs {
        let target = run_dir.join(path);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, doc.text()).unwrap();
    }
}

fn scratch(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("bynk-wire-ref-{name}-{}", std::process::id()))
}

/// The page's worked hashes are what the emitter stamps for the same source.
#[test]
fn the_pages_example_hashes_are_the_emitted_constants() {
    let tmp = scratch("hash");
    compile_example(&tmp);
    let index = fs::read_to_string(tmp.join("run/workers/wire-sample/index.ts")).unwrap();
    let contracts = example_contracts();
    assert_eq!(
        contracts.len(),
        3,
        "the example has three `on call` services"
    );
    for (name, form, hash) in &contracts {
        let needle =
            format!("kind: \"ContractMismatch\", service: \"{name}\", expected: \"{hash}\"");
        assert!(
            index.contains(&needle),
            "the emitter's hash for `{name}` is not the page's `{hash}` (normal form `{form}`)"
        );
    }
    let _ = fs::remove_dir_all(&tmp);
}

/// A string only the emitted code for `check` contains, so its position in
/// the dispatch is where the check is made.
fn marker(check: CallCheck) -> String {
    match check {
        CallCheck::Method => format!("if (method !== \"{}\")", wf::CALL_METHOD),
        CallCheck::Authentication => "__verifyCallSecret(request, env)".to_string(),
        CallCheck::WireVersion => format!("request.headers.get(\"{}\")", wf::WIRE_HEADER),
        CallCheck::UnknownService => "switch (servicePath)".to_string(),
        CallCheck::Contract => format!("request.headers.get(\"{}\")", wf::CONTRACT_HEADER),
        CallCheck::MalformedJson => "globalThis.JSON.parse(__text)".to_string(),
        CallCheck::MissingCaller => format!("request.headers.get(\"{}\")", wf::CALLER_HEADER),
        CallCheck::Arguments => "const __r_".to_string(),
    }
}

/// The emitted call dispatch makes the published checks in the published
/// order. Fixture 216's callee binds `by c: Caller`, so it makes all eight.
#[test]
fn the_emitted_dispatch_makes_the_published_checks_in_order() {
    let golden = repo(
        "bynkc/tests/fixtures/positive/216_cross_context_caller/expected/workers/app-b/index.ts",
    );
    let ts = fs::read_to_string(&golden).unwrap();
    let start = ts
        .find(&format!("path.startsWith(\"{}\")", wf::CALL_PATH_PREFIX))
        .expect("the call dispatch");
    let ts = &ts[start..];
    let mut last = 0;
    for check in wf::CALL_CHECKS {
        let m = marker(check);
        let at = ts
            .find(&m)
            .unwrap_or_else(|| panic!("{check:?}: `{m}` is not in the call dispatch"));
        assert!(
            at > last || last == 0,
            "{check:?} (`{m}`) is made out of the published order"
        );
        last = at;
        if let Some(kind) = check.kind() {
            let body = format!("kind: \"{kind}\"");
            let status = format!("status: {}", check.status());
            let tail = &ts[at..];
            let kind_at = tail
                .find(&body)
                .unwrap_or_else(|| panic!("{check:?}: no `{body}`"));
            assert!(
                tail[kind_at..].find(&status).is_some_and(|s| s < 200),
                "{check:?} answers `{body}` with a status other than {}",
                check.status()
            );
        }
    }
}

/// The runtime's caller sends what the page publishes.
#[test]
fn the_runtime_caller_sends_the_published_request() {
    // The bundled runtime the emitter embeds, not the module it is bundled
    // from, so a stale bundle fails here too.
    let boundary = fs::read_to_string(repo("bynk-emit/src/emitter/runtime.ts")).unwrap();
    for needle in [
        format!(
            "export const WIRE_VERSION_HEADER = \"{}\";",
            wf::WIRE_HEADER
        ),
        format!("export const WIRE_VERSION = \"{}\";", wf::WIRE_VERSION),
        format!(
            "export const CALL_SECRET_HEADER = \"{}\";",
            wf::CALL_SECRET_HEADER
        ),
        format!("`http://internal{}${{servicePath}}`", wf::CALL_PATH_PREFIX),
        format!("method: \"{}\",", wf::CALL_METHOD),
        format!("\"{}\": callerContext,", wf::CALLER_HEADER),
        format!("\"{}\": contractHash,", wf::CONTRACT_HEADER),
        "[WIRE_VERSION_HEADER]: WIRE_VERSION,".to_string(),
        "[CALL_SECRET_HEADER]: secret,".to_string(),
    ] {
        assert!(
            boundary.contains(&needle),
            "bynk-emit/src/emitter/runtime.ts no longer contains `{needle}`"
        );
    }
    assert!(
        wf::ACCEPTED_WIRE_VERSIONS.contains(&wf::WIRE_VERSION),
        "a callee must accept the version its callers speak"
    );
}

fn tool_exists(name: &str) -> bool {
    let finder = if cfg!(windows) { "where" } else { "which" };
    Command::new(finder)
        .arg(name)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn base_command(program: &str) -> Command {
    if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(program);
        c
    } else {
        Command::new(program)
    }
}

fn discover_tsc() -> Option<(String, Vec<String>)> {
    if tool_exists("tsc") {
        return Some(("tsc".to_string(), vec![]));
    }
    if tool_exists("npx") {
        return Some((
            "npx".to_string(),
            vec![
                "--yes".to_string(),
                "-p".to_string(),
                format!("typescript@{}", bynk_emit::TYPESCRIPT_MAJOR_TESTED),
                "tsc".to_string(),
            ],
        ));
    }
    None
}

fn run(program: &str, prefix: &[String], args: &[&str], cwd: &Path) -> (bool, String) {
    let mut cmd = base_command(program);
    cmd.args(prefix).args(args).current_dir(cwd);
    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => return (false, format!("could not launch {program}: {e}")),
    };
    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&output.stderr));
    (output.status.success(), combined)
}

const TSCONFIG_JSON: &str = r#"{
  "compilerOptions": {
    "target": "ES2022",
    "module": "NodeNext",
    "moduleResolution": "NodeNext",
    "strict": true,
    "skipLibCheck": true,
    "outDir": "js",
    "rootDir": ".",
    "lib": ["ES2022", "DOM"]
  },
  "include": ["**/*.ts"]
}
"#;

/// A TypeScript string literal for `s`.
fn ts_str(s: &str) -> String {
    serde_json::to_string(s).unwrap()
}

/// The examples as a TS array the driver walks.
fn examples_ts() -> String {
    let mut out = String::from("[\n");
    for ex in EXAMPLES {
        let outcome = match &ex.outcome {
            Outcome::Accepted { encodes_as } => {
                format!("{{ accepted: {} }}", ts_str(encodes_as))
            }
            Outcome::Rejected { kind, path } => {
                format!("{{ kind: {}, path: {} }}", ts_str(kind), ts_str(path))
            }
        };
        out.push_str(&format!(
            "  {{ codec: {}, json: {}, outcome: {outcome} }},\n",
            ts_str(ex.codec),
            ts_str(ex.json)
        ));
    }
    out.push(']');
    out
}

const DRIVER_TS: &str = r#"import worker from "./workers/wire-sample/index.js";
import * as codecs from "./workers/wire-sample/handlers.js";

function assert(cond: boolean, msg: string): void {
  if (!cond) throw new Error("FAIL: " + msg);
}

const EXAMPLES: any[] = __EXAMPLES__;
const c: any = codecs;
for (const ex of EXAMPLES) {
  const decode = c["__deserialise_" + ex.codec];
  const encode = c["__serialise_" + ex.codec];
  assert(typeof decode === "function", "no codec __deserialise_" + ex.codec);
  const r = decode(JSON.parse(ex.json));
  const what = ex.codec + " " + ex.json;
  if ("accepted" in ex.outcome) {
    assert(r.tag === "Ok", what + " → accepted, got " + JSON.stringify(r));
    const back = JSON.stringify(encode(r.value));
    assert(back === JSON.stringify(JSON.parse(ex.outcome.accepted)), what + " encodes back as " + back);
  } else {
    assert(r.tag === "Err", what + " → rejected, got " + JSON.stringify(r));
    assert(r.error.kind === ex.outcome.kind, what + " → " + ex.outcome.kind + ", got " + JSON.stringify(r.error));
    assert(r.error.path === ex.outcome.path, what + " → at " + ex.outcome.path + ", got " + JSON.stringify(r.error));
  }
}

// The dispatch, against the page's checks table.
const SECRET = "s3cret";
const env: any = { BYNK_CALL_SECRET: SECRET };
const QUOTE = "__QUOTE_HASH__";
const LINE = JSON.parse(__LINE__);

function call(opts: { method?: string; headers?: Record<string, string>; body?: string | null; service?: string }): Request {
  const method = opts.method ?? "POST";
  return new Request("http://internal/_bynk/call/" + (opts.service ?? "quote"), {
    method,
    headers: {
      "content-type": "application/json",
      "X-Bynk-Contract": QUOTE,
      "X-Bynk-Call-Secret": SECRET,
      ...(opts.headers ?? {}),
    },
    body: method === "GET" || method === "HEAD" ? null : (opts.body ?? JSON.stringify({ line: LINE, size: 3 })),
  });
}

async function expectStatus(req: Request, status: number, what: string): Promise<Response> {
  const res = await worker.fetch(req, env);
  assert(res.status === status, what + " → " + status + ", got " + res.status);
  return res;
}

// Success, with and without the wire header.
let res = await expectStatus(call({}), 200, "a well-formed call");
let body: any = await res.json();
assert(body.kind === "Ok" && body.value.kind === "Dot", "the encoded return value, got " + JSON.stringify(body));
await expectStatus(call({ headers: { "X-Bynk-Wire": "1" } }), 200, "wire version 1");

// 1. Method.
res = await expectStatus(call({ method: "GET" }), 405, "a GET");
assert(res.headers.get("allow") === "POST", "405 names the allowed method");
await expectStatus(call({ method: "PUT" }), 405, "a PUT");

// 2. Authentication precedes everything after it.
res = await expectStatus(call({ headers: { "X-Bynk-Call-Secret": "nope", "X-Bynk-Wire": "9" }, service: "nosuch" }), 401, "a wrong secret");
assert((await res.text()) === "Unauthorized", "401 is bare");

// 3. Wire version.
res = await expectStatus(call({ headers: { "X-Bynk-Wire": "2" }, service: "nosuch" }), 400, "wire version 2");
body = await res.json();
assert(body.kind === "UnsupportedWireVersion" && JSON.stringify(body.accepted) === "[1]", "the accepted versions, got " + JSON.stringify(body));

// 4. Unknown service.
res = await expectStatus(call({ service: "nosuch" }), 404, "an unknown service");
assert((await res.text()) === "Not found", "404 body");

// 5. Contract.
res = await expectStatus(call({ headers: { "X-Bynk-Contract": "0000000000000000" }, body: "{" }), 409, "a skewed contract");
body = await res.json();
assert(body.kind === "ContractMismatch" && body.expected === QUOTE, "409 body, got " + JSON.stringify(body));

// 6. Malformed JSON.
for (const bad of ["{", "", "{\"line\": }"]) {
  res = await expectStatus(call({ body: bad }), 400, "malformed body " + JSON.stringify(bad));
  body = await res.json();
  assert(body.kind === "MalformedJson" && body.details === "Invalid request body", "MalformedJson body, got " + JSON.stringify(body));
}

// 7. Missing caller, for a handler that binds `by c: Caller`.
const WHO = "__WHO_HASH__";
for (const caller of [null, ""]) {
  const headers: Record<string, string> = { "X-Bynk-Contract": WHO };
  if (caller !== null) headers["X-Bynk-Caller"] = caller;
  res = await expectStatus(call({ service: "who", headers, body: JSON.stringify({ n: 1, v: "x" }) }), 401, "caller " + JSON.stringify(caller));
  body = await res.json();
  assert(body.kind === "Unauthorized" && body.details === "missing caller identity", "the 401 body, got " + JSON.stringify(body));
}
res = await expectStatus(call({ service: "who", headers: { "X-Bynk-Contract": WHO, "X-Bynk-Caller": "shop.orders" }, body: JSON.stringify({ n: 1, v: "x" }) }), 200, "with a caller");
body = await res.json();
assert(body.kind === "Ok" && body.value === "shop.orders", "the handler reads the caller, got " + JSON.stringify(body));

// 8. Arguments: an inline field refinement, through the dispatch's path.
res = await expectStatus(call({ body: JSON.stringify({ line: { ...LINE, qty: 0 }, size: 3 }) }), 400, "qty 0");
body = await res.json();
assert(body.kind === "RefinementViolation" && body.path === "$.line.qty", "the violation's path, got " + JSON.stringify(body));
res = await expectStatus(call({ body: JSON.stringify({ line: { ...LINE, wait: 0.5 }, size: 3 }) }), 400, "a fractional Duration");
body = await res.json();
assert(body.kind === "StructuralMismatch" && body.path === "$.line.wait", "the Duration's path, got " + JSON.stringify(body));

console.log("ALL OK");
"#;

/// The page's examples and checks, executed against the generated code.
#[test]
fn the_pages_examples_and_checks_hold_on_the_generated_code() {
    let runner = match discover_tsc() {
        Some(r) => r,
        None => {
            eprintln!("\n!!! WIRE-FORMAT REFERENCE VERIFICATION SKIPPED !!!\nno tsc runner.\n");
            if std::env::var(REQUIRE_ENV).is_ok() {
                panic!("{REQUIRE_ENV} is set but no tsc runner was found");
            }
            return;
        }
    };
    if !tool_exists("node") {
        eprintln!("\n!!! WIRE-FORMAT REFERENCE VERIFICATION SKIPPED !!!\n`node` not on PATH.\n");
        if std::env::var(REQUIRE_ENV).is_ok() {
            panic!("{REQUIRE_ENV} is set but `node` was not found");
        }
        return;
    }

    let tmp = scratch("run");
    compile_example(&tmp);
    let run_dir = tmp.join("run");
    let quote = example_contracts()
        .into_iter()
        .find(|(name, _, _)| name == "quote")
        .expect("the example's `quote` service")
        .2;
    let who = example_contracts()
        .into_iter()
        .find(|(name, _, _)| name == "who")
        .expect("the example's `who` service")
        .2;
    let line = match &EXAMPLES[0].outcome {
        Outcome::Accepted { .. } => EXAMPLES[0].json,
        Outcome::Rejected { .. } => panic!("the first example is a whole, accepted `Line`"),
    };
    let driver = DRIVER_TS
        .replace("__EXAMPLES__", &examples_ts())
        .replace("__QUOTE_HASH__", &quote)
        .replace("__WHO_HASH__", &who)
        .replace("__LINE__", &ts_str(line));
    fs::write(run_dir.join("driver.ts"), driver).unwrap();
    fs::write(run_dir.join("tsconfig.json"), TSCONFIG_JSON).unwrap();
    fs::write(run_dir.join("package.json"), r#"{ "type": "module" }"#).unwrap();

    let (ok, out) = run(&runner.0, &runner.1, &["-p", "tsconfig.json"], &run_dir);
    assert!(ok, "tsc failed on the generated example:\n{out}");
    let (ok, out) = run("node", &[], &["js/driver.js"], &run_dir);
    assert!(
        ok && out.contains("ALL OK"),
        "the wire-format page disagrees with the generated code:\n{out}"
    );
    let _ = fs::remove_dir_all(&tmp);
}
