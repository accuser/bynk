//! First-party standard adapters embedded in the toolchain (v0.17 §4.2).
//!
//! The `karn` conformance surface is shipped with the compiler rather than
//! authored by the user. When a project `consumes karn`, the driver injects the
//! adapter source below as a synthetic unit and provides its binding for the
//! selected [`Platform`]. The `karn` root namespace is reserved
//! (`karn.namespace.reserved`) so user code can never collide with it.

/// The deploy platform — a selection axis distinct from the `--target
/// {bundle,workers}` emit mode (§6.2). It chooses which `karn-<platform>.ts`
/// binding is linked for the `karn` surface. v0.17 shipped `cloudflare`;
/// v0.18 adds `node`, making the axis observable (and giving v0.19's
/// platform-lock enforcement a second platform to fire against).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Platform {
    #[default]
    Cloudflare,
    Node,
}

impl Platform {
    /// The output filename of this platform's `karn` binding module.
    pub fn karn_binding_filename(self) -> &'static str {
        match self {
            Platform::Cloudflare => "karn-cloudflare.ts",
            Platform::Node => "karn-node.ts",
        }
    }

    /// The TypeScript source of this platform's `karn` binding.
    pub fn karn_binding_source(self) -> &'static str {
        match self {
            Platform::Cloudflare => KARN_CLOUDFLARE_BINDING,
            Platform::Node => KARN_NODE_BINDING,
        }
    }

    /// The platform's stable name (for the `--platform` flag and diagnostics).
    pub fn as_str(self) -> &'static str {
        match self {
            Platform::Cloudflare => "cloudflare",
            Platform::Node => "node",
        }
    }
}

/// The unit name of the reserved first-party surface adapter.
pub const KARN_UNIT: &str = "karn";

/// The unit name of the first-party Cloudflare platform adapter (v0.19,
/// decision 0026): inside the reserved `karn.*` prefix, so no separate
/// reservation rule is needed. The surface unit `karn` stays the portability
/// marker; `karn.<platform>` units are the platform-locked ones.
pub const CLOUDFLARE_UNIT: &str = "karn.cloudflare";

/// The fixed Worker binding name for the Kv namespace (decision C1, v0.19):
/// one namespace, one `[[kv_namespaces]]` stanza, one `env.KV` field.
pub const KV_BINDING_NAME: &str = "KV";

/// v0.18 decision 0021 / v0.19 decision 0025: which first-party provider
/// classes take the Worker `env` as a constructor argument, keyed by
/// (unit, provider class). `SecretsProvider` accepts an optional env with a
/// `globalThis` probe fallback; `WorkersKv` *requires* env on use — KV
/// namespaces exist only on the Worker `env`, never on `globalThis`.
pub fn provider_takes_env(unit: &str, provider: &str) -> bool {
    matches!(
        (unit, provider),
        (KARN_UNIT, "SecretsProvider") | (CLOUDFLARE_UNIT, "WorkersKv")
    )
}

/// v0.19 decision 0024: which first-party units are **platform-native** —
/// consuming one locks its deployment unit to the returned platform. This is
/// the metadata that drives effective-platform computation, `env` resource
/// typing, and `wrangler.toml` stanza derivation; no user-facing marker
/// syntax exists (additive later, when third-party platform adapters become
/// a goal).
pub fn platform_of(unit: &str) -> Option<Platform> {
    match unit {
        CLOUDFLARE_UNIT => Some(Platform::Cloudflare),
        _ => None,
    }
}

/// The reserved `karn` conformance-surface adapter (env-free core). It has no
/// `binding` clause — the toolchain supplies one per platform (see
/// [`Platform::karn_binding_source`]).
pub const KARN_ADAPTER_SRC: &str = r#"adapter karn {
  exports capability  { Clock, Random, Logger, Fetch, Secrets }
  exports transparent { Uuid, Method, FetchError, Request, Response }

  type Uuid = String where Matches("[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")

  type Method     = enum { Get, Post, Put, Delete }
  type FetchError = enum { Network, Timeout }

  type Request = {
    method: Method,
    url: String,
    contentType: Option[String],
    authorization: Option[String],
    body: Option[String],
  }

  type Response = {
    status: Int,
    body: String,
  }

  capability Clock {
    fn now() -> Effect[Int]
  }

  capability Random {
    fn uuid() -> Effect[Uuid]
    fn int(lo: Int, hi: Int) -> Effect[Int]
  }

  capability Logger {
    fn info(msg: String) -> Effect[()]
    fn error(msg: String) -> Effect[()]
  }

  capability Fetch {
    fn send(req: Request) -> Effect[Result[Response, FetchError]]
  }

  capability Secrets {
    fn get(name: String) -> Effect[Option[String]]
  }

  provides Clock   = ClockProvider
  provides Random  = RandomProvider
  provides Logger  = LoggerProvider
  provides Fetch   = FetchProvider
  provides Secrets = SecretsProvider
}
"#;

/// The Cloudflare binding for the `karn` surface. Implements the canonical
/// provider symbols against the platform host API. The refined `Uuid` is built
/// through its emitted validating `.of` constructor (§4.4), treating the
/// unreachable `Err` as a bug rather than trusting the value away.
const KARN_CLOUDFLARE_BINDING: &str = r#"// Generated by karnc — do not edit by hand.
// karn surface binding for the `cloudflare` platform.
import type { Clock, Fetch, Logger, Random, Secrets } from "./karn.js";
import type { Request as FetchRequest, Response as FetchResponse } from "./karn.js";
import { FetchError, Uuid } from "./karn.js";
import { Err, None, Ok, Some, type Option, type Result } from "./runtime.js";

export class ClockProvider implements Clock {
  async now(): Promise<number> {
    return Date.now();
  }
}

export class RandomProvider implements Random {
  async uuid(): Promise<Uuid> {
    const r = Uuid.of(crypto.randomUUID());
    if (r.tag === "Err") {
      throw new Error("unreachable: crypto.randomUUID() yields a valid UUID");
    }
    return r.value;
  }
  async int(lo: number, hi: number): Promise<number> {
    return lo + Math.floor(Math.random() * (hi - lo));
  }
}

export class LoggerProvider implements Logger {
  async info(msg: string): Promise<void> {
    console.log(msg);
  }
  async error(msg: string): Promise<void> {
    console.error(msg);
  }
}

export class FetchProvider implements Fetch {
  async send(req: FetchRequest): Promise<Result<FetchResponse, FetchError>> {
    const headers: Record<string, string> = {};
    if (req.contentType.tag === "Some") {
      headers["content-type"] = req.contentType.value;
    }
    if (req.authorization.tag === "Some") {
      headers["authorization"] = req.authorization.value;
    }
    try {
      const res = await fetch(req.url, {
        method: req.method.tag.toUpperCase(),
        headers,
        body: req.body.tag === "Some" ? req.body.value : undefined,
      });
      return Ok({ status: res.status, body: await res.text() });
    } catch (e) {
      const name = e instanceof Error ? e.name : "";
      return Err(name === "TimeoutError" || name === "AbortError" ? FetchError.Timeout : FetchError.Network);
    }
  }
}

export class SecretsProvider implements Secrets {
  // Decision [B]: explicit env first (workers compose passes it), then a
  // `globalThis` probe of `process.env` (bundle under node) — never bare
  // `process`, which would demand @types/node at the tsc gate.
  constructor(private env?: unknown) {}
  async get(name: string): Promise<Option<string>> {
    const fromEnv = (this.env as Record<string, unknown> | undefined)?.[name];
    if (typeof fromEnv === "string") {
      return Some(fromEnv);
    }
    const v = (globalThis as any).process?.env?.[name];
    return typeof v === "string" ? Some(v) : None;
  }
}
"#;

/// The Node (≥ 18) binding for the `karn` surface (v0.18). Deliberately
/// near-identical to the cloudflare binding: `Date.now`, the global
/// `crypto`/`fetch`, and `console` are the same host API on both runtimes —
/// which is exactly the ambient-surface portability claim (spec §4.2). The
/// `SecretsProvider` reads `process.env` through the same `globalThis` probe
/// (never bare `process`, which would demand @types/node at the tsc gate).
const KARN_NODE_BINDING: &str = r#"// Generated by karnc — do not edit by hand.
// karn surface binding for the `node` platform.
import type { Clock, Fetch, Logger, Random, Secrets } from "./karn.js";
import type { Request as FetchRequest, Response as FetchResponse } from "./karn.js";
import { FetchError, Uuid } from "./karn.js";
import { Err, None, Ok, Some, type Option, type Result } from "./runtime.js";

export class ClockProvider implements Clock {
  async now(): Promise<number> {
    return Date.now();
  }
}

export class RandomProvider implements Random {
  async uuid(): Promise<Uuid> {
    const r = Uuid.of(crypto.randomUUID());
    if (r.tag === "Err") {
      throw new Error("unreachable: crypto.randomUUID() yields a valid UUID");
    }
    return r.value;
  }
  async int(lo: number, hi: number): Promise<number> {
    return lo + Math.floor(Math.random() * (hi - lo));
  }
}

export class LoggerProvider implements Logger {
  async info(msg: string): Promise<void> {
    console.log(msg);
  }
  async error(msg: string): Promise<void> {
    console.error(msg);
  }
}

export class FetchProvider implements Fetch {
  async send(req: FetchRequest): Promise<Result<FetchResponse, FetchError>> {
    const headers: Record<string, string> = {};
    if (req.contentType.tag === "Some") {
      headers["content-type"] = req.contentType.value;
    }
    if (req.authorization.tag === "Some") {
      headers["authorization"] = req.authorization.value;
    }
    try {
      const res = await fetch(req.url, {
        method: req.method.tag.toUpperCase(),
        headers,
        body: req.body.tag === "Some" ? req.body.value : undefined,
      });
      return Ok({ status: res.status, body: await res.text() });
    } catch (e) {
      const name = e instanceof Error ? e.name : "";
      return Err(name === "TimeoutError" || name === "AbortError" ? FetchError.Timeout : FetchError.Network);
    }
  }
}

export class SecretsProvider implements Secrets {
  // Decision [B]: explicit env first, then the `globalThis` probe of
  // `process.env` — on node the probe is the normal path.
  constructor(private env?: unknown) {}
  async get(name: string): Promise<Option<string>> {
    const fromEnv = (this.env as Record<string, unknown> | undefined)?.[name];
    if (typeof fromEnv === "string") {
      return Some(fromEnv);
    }
    const v = (globalThis as any).process?.env?.[name];
    return typeof v === "string" ? Some(v) : None;
  }
}
"#;

/// The first-party Cloudflare platform adapter (v0.19): the platform's real
/// infrastructure capabilities, as they are — no portable intersection
/// (decision 0016). The v0.19 surface is the minimal, collection-free `Kv`
/// (decision 0023); `Kv.list`, structured values, and `Queue` are the v0.22
/// extension. Like the `karn` surface it has no `binding` clause — the
/// toolchain supplies the binding.
pub const CLOUDFLARE_ADAPTER_SRC: &str = r#"adapter karn.cloudflare {
  exports capability { Kv }

  capability Kv {
    fn get(key: String) -> Effect[Option[String]]
    fn put(key: String, value: String) -> Effect[()]
    fn delete(key: String) -> Effect[()]
  }

  provides Kv = WorkersKv
}
"#;

/// The output path of the Cloudflare platform adapter's binding module,
/// beside the adapter's emitted `karn/cloudflare.ts` (distinct from the
/// `karn` *surface*'s per-platform `karn-cloudflare.ts`).
pub const CLOUDFLARE_BINDING_FILENAME: &str = "karn/cloudflare.binding.ts";

/// The Cloudflare platform adapter's binding. `WorkersKv` reads the Worker
/// `env` explicitly (decision 0025): KV namespaces exist only on `env` —
/// there is no `globalThis` path — so a missing binding is a clear runtime
/// error rather than a silent fallback.
const CLOUDFLARE_BINDING: &str = r#"// Generated by karnc — do not edit by hand.
// karn.cloudflare platform adapter binding.
import type { Kv } from "./cloudflare.js";
import { None, Some, type KVNamespace, type Option } from "../runtime.js";

export class WorkersKv implements Kv {
  constructor(private env?: unknown) {}

  private ns(): KVNamespace {
    const kv = (this.env as { KV?: KVNamespace } | undefined)?.KV;
    if (!kv) {
      throw new Error(
        "karn.cloudflare.Kv requires a KV namespace binding (env.KV) — deploy with the generated [[kv_namespaces]] wrangler stanza",
      );
    }
    return kv;
  }

  async get(key: string): Promise<Option<string>> {
    const v = await this.ns().get(key);
    return v === null ? None : Some(v);
  }

  async put(key: string, value: string): Promise<void> {
    await this.ns().put(key, value);
  }

  async delete(key: string): Promise<void> {
    await this.ns().delete(key);
  }
}
"#;

/// The toolchain-supplied binding for the Cloudflare platform adapter.
pub fn cloudflare_binding_source() -> &'static str {
    CLOUDFLARE_BINDING
}
