// `cloudflare.binding`: the `bynk.cloudflare` adapter's `WorkersKv` binding,
// tested against a KV namespace fake (standard-library S2, #1952), and its
// `WorkersService` (interop I5, #1972) against a Service Binding fake.
//
// The fake follows Cloudflare's documented `KVNamespace` contract, not the
// binding: `get` resolves `null` for an absent key; `list` returns keys in
// lexicographic order, at most `limit` per page (1000 by default), with
// `list_complete: false` and a `cursor` while more remain, and no cursor on
// the last page; `put` takes `{ expirationTtl }` in seconds. Pages are small
// here so that draining several of them is exercised. Whether the fake matches
// the platform is what `bynkc/tests/binding_workerd.rs` checks, on `workerd`.

import { test } from "node:test";
import assert from "node:assert/strict";
import { WorkersKv, WorkersService } from "./bynk/cloudflare.binding.js";
import { Method, FetchError } from "./bynk.js";
import { Some, None } from "./runtime.js";

function fakeKv(pageSize = 2) {
  const entries = new Map();
  const puts = [];
  return {
    entries,
    puts,
    async get(key) {
      return entries.has(key) ? entries.get(key) : null;
    },
    async put(key, value, options) {
      puts.push({ key, value, options });
      entries.set(key, value);
    },
    async delete(key) {
      entries.delete(key);
    },
    async list({ prefix, cursor, limit } = {}) {
      const keys = [...entries.keys()].filter((k) => !prefix || k.startsWith(prefix)).sort();
      const start = cursor === undefined ? 0 : Number(cursor);
      const size = Math.min(limit ?? 1000, pageSize);
      const page = keys.slice(start, start + size).map((name) => ({ name }));
      const next = start + size;
      return next < keys.length
        ? { keys: page, list_complete: false, cursor: String(next) }
        : { keys: page, list_complete: true };
    },
  };
}

const t = (claim, fn) => test(`cloudflare.binding ${claim}`, fn);

t("WorkersKv.get: an absent key is None, a present one Some", async () => {
  const kv = fakeKv();
  kv.entries.set("present", "v");
  const store = new WorkersKv({ KV: kv });
  assert.deepEqual(await store.get("absent"), None);
  assert.deepEqual(await store.get("present"), Some("v"));
});

t("WorkersKv.put: stores the value with no expiry option", async () => {
  const kv = fakeKv();
  const store = new WorkersKv({ KV: kv });
  await store.put("k", "v1");
  await store.put("k", "v2");
  assert.deepEqual(await store.get("k"), Some("v2"));
  assert.equal(kv.puts.length, 2);
  assert.equal(kv.puts[1].options, undefined);
});

t("WorkersKv.putTtl: passes the TTL through as expirationTtl seconds", async () => {
  const kv = fakeKv();
  const store = new WorkersKv({ KV: kv });
  await store.putTtl("k", "v", 120);
  assert.deepEqual(kv.puts, [{ key: "k", value: "v", options: { expirationTtl: 120 } }]);
  assert.deepEqual(await store.get("k"), Some("v"));
});

t("WorkersKv.delete: removes the key, and deleting an absent key is not an error", async () => {
  const kv = fakeKv();
  kv.entries.set("k", "v");
  const store = new WorkersKv({ KV: kv });
  await store.delete("k");
  assert.deepEqual(await store.get("k"), None);
  await store.delete("never-there");
});

t("WorkersKv.list: drains every page, filtered by prefix, in key order", async () => {
  const kv = fakeKv(2);
  for (const k of ["user:3", "user:1", "order:1", "user:2", "user:5", "user:4"]) {
    kv.entries.set(k, "x");
  }
  const store = new WorkersKv({ KV: kv });
  assert.deepEqual(await store.list(Some("user:")), ["user:1", "user:2", "user:3", "user:4", "user:5"]);
  assert.deepEqual(await store.list(None), ["order:1", "user:1", "user:2", "user:3", "user:4", "user:5"]);
  assert.deepEqual(await store.list(Some("nothing:")), []);
});

t("WorkersKv.get: without an env.KV binding, says so rather than answering", async () => {
  await assert.rejects(new WorkersKv({}).get("k"), /requires a KV namespace binding \(env\.KV\)/);
  await assert.rejects(new WorkersKv().get("k"), /env\.KV/);
});

// S4a (#1966): a derived capability's instance (`capability Sessions = Kv`)
// reads its own binding, named at construction; the bare `Kv` keeps `KV`.
t("WorkersKv: a named binding reads env[binding], apart from KV", async () => {
  const bare = fakeKv();
  const sessions = fakeKv();
  const env = { KV: bare, KV_SESSIONS: sessions };
  await new WorkersKv(env, "KV_SESSIONS").put("k", "instance");
  await new WorkersKv(env).put("k", "bare");
  assert.deepEqual(await new WorkersKv(env, "KV_SESSIONS").get("k"), Some("instance"));
  assert.deepEqual(await new WorkersKv(env).get("k"), Some("bare"));
  assert.equal(sessions.entries.get("k"), "instance");
  assert.equal(bare.entries.get("k"), "bare");
});

t("WorkersKv: a missing named binding names it", async () => {
  await assert.rejects(
    new WorkersKv({ KV: fakeKv() }, "KV_SESSIONS").get("k"),
    /requires a KV namespace binding \(env\.KV_SESSIONS\)/,
  );
});

// Interop I5 (#1972): a Service Binding fake following Cloudflare's
// `Fetcher` contract as the generated `Env` types it: `fetch(request)` with
// one `Request`, answering a `Response`. Records what it was sent.
function fakeService(answer = () => new Response("ok", { status: 200 })) {
  const sent = [];
  return {
    sent,
    async fetch(request) {
      sent.push({
        method: request.method,
        url: request.url,
        headers: Object.fromEntries(request.headers),
        body: request.body === null ? null : await request.text(),
      });
      return answer(request);
    },
  };
}

const serviceRequest = (overrides = {}) => ({
  method: Method.Get,
  url: "https://legacy-billing/charge",
  contentType: None,
  authorization: None,
  headers: new Map(),
  body: None,
  ...overrides,
});

t("WorkersService.send: sends over env[binding], not the network, and reads the response", async () => {
  const billing = fakeService(() => new Response("charged", { status: 201, headers: { etag: "v1" } }));
  const globalFetch = globalThis.fetch;
  globalThis.fetch = () => {
    throw new Error("the global fetch must not be used");
  };
  try {
    const res = await new WorkersService({ SERVICE_BILLING: billing }, "SERVICE_BILLING").send(
      serviceRequest({ method: Method.Post, contentType: Some("text/plain"), body: Some("a") }),
    );
    assert.equal(res.tag, "Ok");
    assert.equal(res.value.status, 201);
    assert.equal(res.value.body, "charged");
    assert.equal(res.value.headers.get("etag"), "v1");
  } finally {
    globalThis.fetch = globalFetch;
  }
  assert.equal(billing.sent.length, 1);
  assert.equal(billing.sent[0].method, "POST");
  assert.equal(billing.sent[0].url, "https://legacy-billing/charge");
  assert.equal(billing.sent[0].headers["content-type"], "text/plain");
  assert.equal(billing.sent[0].body, "a");
});

t("WorkersService.send: a status the Worker returns, 5xx included, is Ok", async () => {
  const billing = fakeService(() => new Response("down", { status: 503 }));
  const res = await new WorkersService({ SERVICE_BILLING: billing }, "SERVICE_BILLING").send(serviceRequest());
  assert.equal(res.tag, "Ok");
  assert.equal(res.value.status, 503);
});

t("WorkersService.send: a refused header is Err(InvalidHeader), and nothing is sent", async () => {
  const billing = fakeService();
  const res = await new WorkersService({ SERVICE_BILLING: billing }, "SERVICE_BILLING").send(
    serviceRequest({ headers: new Map([["host", "elsewhere"]]) }),
  );
  assert.deepEqual(res, { tag: "Err", error: FetchError.InvalidHeader });
  assert.equal(billing.sent.length, 0);
});

t("WorkersService.send: a call the binding fails is Err(Network)", async () => {
  const billing = {
    async fetch() {
      throw new TypeError("internal error");
    },
  };
  const res = await new WorkersService({ SERVICE_BILLING: billing }, "SERVICE_BILLING").send(serviceRequest());
  assert.deepEqual(res, { tag: "Err", error: FetchError.Network });
});

t("WorkersService.send: each instance reads its own binding", async () => {
  const billing = fakeService();
  const ledger = fakeService();
  const env = { SERVICE_BILLING: billing, SERVICE_LEDGER: ledger };
  await new WorkersService(env, "SERVICE_LEDGER").send(serviceRequest());
  assert.equal(billing.sent.length, 0);
  assert.equal(ledger.sent.length, 1);
});

t("WorkersService.send: a missing binding names it", async () => {
  await assert.rejects(
    new WorkersService({}, "SERVICE_BILLING").send(serviceRequest()),
    /requires a Service Binding \(env\.SERVICE_BILLING\)/,
  );
});
