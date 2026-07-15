# Changelog

## Unreleased — project renamed from **Karn** to **Bynk**

The project, its toolchain, and its in-language surface were renamed from
**Karn** to **Bynk**. This is a **breaking** change for existing sources.

### Toolchain

- The compiler binary `karnc` is now **`bynkc`**; the driver `karn` is now
  **`bynk`** (e.g. `bynk build`, `bynk test`, `bynk doctor`). The language
  server `karnc-lsp` is now **`bynkc-lsp`**.
- The project manifest `karn.toml` is now **`bynk.toml`**.
- The source file extension `.karn` is now **`.bynk`**.
- `bynk deploy` provisions the KV namespace required by a Cloudflare Worker,
  stores its id in committed `bynk.deploy.lock`, and pushes through Wrangler. It
  supports a non-mutating `--dry-run` plan, JSON output, confirmation / `--yes`,
  and idempotent re-deploys (#583).
- `bynk deploy` ships **every** context of a project, ordered so that each
  Worker is uploaded after the Workers it binds to — Cloudflare resolves a
  Service Binding at upload and rejects one whose target does not yet exist, so
  the order is a correctness requirement. It previously refused any project with
  more than one context. `--context NAME` re-pushes one context and reports
  (rather than pushes into) a dependency that was never deployed. A failure
  stops the run and keeps what landed; a re-run resumes in the same order
  (#601). **Breaking (pre-1.0):** `--format json` describes every context, so
  the top-level `worker`, `kv`, and `deploy` fields are replaced by a `contexts`
  array alongside the resolved `order` — read `.contexts[0].worker` for
  `.worker`, and `.contexts[0].action` (`deploy` or `redeploy`) for `.deploy`,
  which was always `true`.
- `bynk dev` serves **every** context of a multi-context project at once, with
  the Cloudflare Service Bindings between them wired, so cross-context calls
  resolve locally — one `wrangler dev` per context, connected through wrangler's
  dev registry. It previously served one context and failed a multi-context
  project as ambiguous. `--context` is now repeatable and narrows to a subset;
  each context gets its own port from `--base-port` (and its own inspector port
  from `--inspect-port` under `--inspect`), so `--port` is no longer accepted
  through the `--` passthrough where the driver allocates it (#552).

### In-language reserved surface (breaking)

The reserved namespace `karn` is renamed to **`bynk`**. Update your sources:

- `consumes karn { … }` → `consumes bynk { … }`
- platform adapters `karn.cloudflare` / `karn.node` / … → `bynk.cloudflare` /
  `bynk.node` / …
- stdlib units `karn.list` / `karn.map` / `karn.string` → `bynk.list` /
  `bynk.map` / `bynk.string`

### Diagnostics & runtime

- Every diagnostic code is reprefixed `karn.*` → **`bynk.*`** (e.g.
  `karn.namespace.reserved` → `bynk.namespace.reserved`).
- The internal HTTP dispatch prefix `/_karn/` is now **`/_bynk/`** and the
  cross-context caller header `X-Karn-Caller` is now **`X-Bynk-Caller`**.

### Migrating a project

1. Rename `karn.toml` → `bynk.toml` and every `*.karn` source → `*.bynk`.
2. Replace `consumes karn` with `consumes bynk` and every `karn.<platform>` /
   `karn.<stdlib>` reference with its `bynk.<…>` equivalent.
3. Recompile with `bynkc` (or `bynk build`).

## v0.142.0 — request body-size limits & numeric digit separators

Two features ship together (ADRs 0165 & 0166).

### Request body-size limits for `from http` services (ADR 0165)

A body-taking route (`POST`/`PUT`/`PATCH`) can now bound its request body size:

- A service declares a byte ceiling with a **`limits { maxBody: <Int> }`** section
  in header position (beside `cors { }` / `security { }`), overridable per route
  with a **`@limit(maxBody: <Int>)`** handler annotation (the `@cache` placement;
  valid **only** on `POST`/`PUT`/`PATCH`).
- A capped route rejects a request whose `Content-Length` exceeds the cap with a
  synthesised **`413 PayloadTooLarge`** (`{ kind: "PayloadTooLarge", details: … }`),
  produced **before the body is read** and before the `by`/Bearer auth seam — the
  boundary posture of the method-semantics `405`. It reuses the existing `413`
  status, so the closed `HttpResult` registry is unchanged; the `413` is
  CORS/security-header-stamped so a cross-origin caller can read it.
- Precedence is **route `@limit` → service `limits` → none**. With neither, a route
  has **no cap** and emits byte-for-byte unchanged output — the feature is opt-in.
- Enforcement is a `Content-Length` fast-reject (not a hard guarantee — the header
  can be absent for a chunked transfer or spoofed), pairing with the Workers
  platform cap. A streamed-read cap is a named follow-on.
- `maxBody` is a positive `Int` byte count (`26_214_400` for 25 MiB). A byte `Size`
  literal (`1.mb`) is a named follow-on.

### Numeric digit separators (ADR 0166)

`Int` and `Float` literals now admit an `_` digit separator between digit groups
(`1_048_576`, `1_000.5`) — never leading, trailing, or doubled. The separators are
stripped before the value is parsed (purely visual), and the as-written lexeme is
preserved so `bynk fmt` keeps the author's grouping. Motivated by `maxBody`'s large
byte counts, but applies language-wide.

Closes #494.
