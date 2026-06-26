//! Behavioural test for the storage-track `store`/`Cell` emission (ADR 0109).
//!
//! Snapshots (the e2e golden) prove the emitted *shape*; this proves the
//! *runtime semantics* on the generated code — the same properties the ADR 0109
//! spike validated on a hand-written Durable Object, now on the emitter's output:
//!
//!   - a `:=` write persists across handler invocations;
//!   - a read after a `:=` in the same handler sees the written value
//!     (read-your-writes via the in-memory working state);
//!   - a handler whose committed state violates an invariant throws
//!     `InvariantViolation` and persists **nothing** (atomic revert — the gate
//!     runs before the durable write).
//!
//! It compiles a `Counter` agent in-process, then `tsc`-compiles the emitted
//! module + a driver and runs it under `node` against a fake Durable Object
//! storage. Like the tsc-verification stage it skips loudly when no TypeScript
//! toolchain is present; `BYNK_REQUIRE_TSC=1` turns the skip into a failure.

use std::fs;
use std::path::Path;
use std::process::Command;

const REQUIRE_ENV: &str = "BYNK_REQUIRE_TSC";

fn base_command(program: &str) -> Command {
    if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(program);
        c
    } else {
        Command::new(program)
    }
}

fn tool_exists(name: &str) -> bool {
    let finder = if cfg!(windows) { "where" } else { "which" };
    Command::new(finder)
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn discover_tsc() -> Option<(String, Vec<String>)> {
    if tool_exists("tsc") {
        return Some(("tsc".to_string(), vec![]));
    }
    if tool_exists("npx") {
        return Some((
            "npx".to_string(),
            vec![
                "-y".to_string(),
                "typescript@5".to_string(),
                "tsc".to_string(),
            ],
        ));
    }
    None
}

fn run(program: &str, prefix: &[String], args: &[&str], cwd: &Path) -> (bool, String) {
    let mut cmd = base_command(program);
    for p in prefix {
        cmd.arg(p);
    }
    for a in args {
        cmd.arg(a);
    }
    cmd.current_dir(cwd);
    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => return (false, format!("could not launch {program}: {e}")),
    };
    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&output.stderr));
    (output.status.success(), combined)
}

const SOURCE: &str = "context shop\n\
\n\
agent Counter {\n\
\x20 key id: String\n\
\x20 store count: Cell[Int] = 0\n\
\n\
\x20 invariant nonneg: count >= 0\n\
\n\
\x20 on call set(n: Int) -> Effect[()] {\n\
\x20   count := n\n\
\x20   Effect.pure(())\n\
\x20 }\n\
\x20 on call get() -> Effect[Int] {\n\
\x20   count\n\
\x20 }\n\
\x20 on call setAndGet(n: Int) -> Effect[Int] {\n\
\x20   count := n\n\
\x20   count\n\
\x20 }\n\
}\n";

const DRIVER_TS: &str = r#"
import { Counter } from "./shop.js";

function assert(cond: boolean, msg: string): void {
  if (!cond) {
    throw new Error(`assertion failed: ${msg}`);
  }
}

// A fake Durable Object state: an in-memory key/value storage with the two
// methods the emitted agent uses.
function fakeState() {
  const m = new Map<string, unknown>();
  return {
    storage: {
      async get(key: string): Promise<unknown> {
        return m.get(key);
      },
      async put(key: string, value: unknown): Promise<void> {
        m.set(key, value);
      },
    },
  };
}

const c = new Counter(fakeState() as never);

// 1) A `:=` write persists, and is visible to a later handler.
await c.set(5, {});
assert((await c.get({})) === 5, "a `:=` write persists across handlers");

// 2) Read-your-writes: a read after `:=` in the same handler sees the value.
const r = await c.setAndGet(7, {});
assert(r === 7, "read-your-writes within a handler");
assert((await c.get({})) === 7, "the setAndGet write persisted");

// 3) Atomic revert: a commit that violates the invariant throws, and the
//    offending write does not persist.
let threw = false;
try {
  await c.set(-1, {});
} catch (e) {
  threw = String((e as { message?: string }).message ?? e).includes("InvariantViolation");
}
assert(threw, "an invariant-violating commit throws InvariantViolation");
assert((await c.get({})) === 7, "atomic revert: the violating write never persisted");

console.log("ALL OK");
"#;

const MAP_SOURCE: &str = "context shop\n\
\n\
agent Cart {\n\
\x20 key id: String\n\
\x20 store items: Map[String, Int]\n\
\n\
\x20 on call add(k: String, n: Int) -> Effect[()] {\n\
\x20   let _ <- items.put(k, n)\n\
\x20   Effect.pure(())\n\
\x20 }\n\
\x20 on call fetch(k: String) -> Effect[Option[Int]] {\n\
\x20   let r <- items.get(k)\n\
\x20   Effect.pure(r)\n\
\x20 }\n\
\x20 on call inc(k: String) -> Effect[()] {\n\
\x20   let _ <- items.upsert(k, 0, (x) => x + 1)\n\
\x20   Effect.pure(())\n\
\x20 }\n\
\x20 on call bumpStrict(k: String) -> Effect[()] {\n\
\x20   let _ <- items.update(k, (x) => x + 1)\n\
\x20   Effect.pure(())\n\
\x20 }\n\
\x20 on call drop(k: String) -> Effect[()] {\n\
\x20   let _ <- items.remove(k)\n\
\x20   Effect.pure(())\n\
\x20 }\n\
\x20 on call count() -> Effect[Int] {\n\
\x20   let n <- items.size()\n\
\x20   Effect.pure(n)\n\
\x20 }\n\
}\n";

const MAP_DRIVER_TS: &str = r#"
import { Cart } from "./shop.js";

function assert(cond: boolean, msg: string): void {
  if (!cond) {
    throw new Error(`assertion failed: ${msg}`);
  }
}

function fakeState() {
  const m = new Map<string, unknown>();
  return {
    storage: {
      async get(key: string): Promise<unknown> { return m.get(key); },
      async put(key: string, value: unknown): Promise<void> { m.set(key, value); },
    },
  };
}

const c = new Cart(fakeState() as never);

// put + get
await c.add("a", 5, {});
let r = await c.fetch("a", {});
assert(r.tag === "Some" && r.value === 5, "put then get");

// upsert on existing, then default-if-absent
await c.inc("a", {});
r = await c.fetch("a", {});
assert(r.tag === "Some" && r.value === 6, "upsert on an existing key");
await c.inc("b", {});
r = await c.fetch("b", {});
assert(r.tag === "Some" && r.value === 1, "upsert default-if-absent");

assert((await c.count({})) === 2, "size counts entries");

// remove
await c.drop("a", {});
r = await c.fetch("a", {});
assert(r.tag === "None", "remove deletes the entry");
assert((await c.count({})) === 1, "size after remove");

// update on an absent key faults — and nothing commits (atomic revert)
let threw = false;
try {
  await c.bumpStrict("missing", {});
} catch (e) {
  threw = String((e as { message?: string }).message ?? e).includes("Map.update: key absent");
}
assert(threw, "update on an absent key throws");
assert((await c.count({})) === 1, "atomic revert: the faulting update left the map unchanged");

console.log("ALL OK");
"#;

const SET_SOURCE: &str = "context shop\n\
\n\
agent Tags {\n\
\x20 key id: String\n\
\x20 store tags: Set[String]\n\
\n\
\x20 on call add(t: String) -> Effect[()] {\n\
\x20   let _ <- tags.add(t)\n\
\x20   Effect.pure(())\n\
\x20 }\n\
\x20 on call drop(t: String) -> Effect[()] {\n\
\x20   let _ <- tags.remove(t)\n\
\x20   Effect.pure(())\n\
\x20 }\n\
\x20 on call has(t: String) -> Effect[Bool] {\n\
\x20   let r <- tags.contains(t)\n\
\x20   Effect.pure(r)\n\
\x20 }\n\
\x20 on call count() -> Effect[Int] {\n\
\x20   let n <- tags.size()\n\
\x20   Effect.pure(n)\n\
\x20 }\n\
}\n";

const SET_DRIVER_TS: &str = r#"
import { Tags } from "./shop.js";

function assert(cond: boolean, msg: string): void {
  if (!cond) {
    throw new Error(`assertion failed: ${msg}`);
  }
}

function fakeState() {
  const m = new Map<string, unknown>();
  return {
    storage: {
      async get(key: string): Promise<unknown> { return m.get(key); },
      async put(key: string, value: unknown): Promise<void> { m.set(key, value); },
    },
  };
}

const c = new Tags(fakeState() as never);

// add + contains
assert((await c.has("x", {})) === false, "absent before add");
await c.add("x", {});
assert((await c.has("x", {})) === true, "contains after add");

// idempotent add
await c.add("x", {});
assert((await c.count({})) === 1, "add is idempotent");

await c.add("y", {});
assert((await c.count({})) === 2, "size counts members");

// remove
await c.drop("x", {});
assert((await c.has("x", {})) === false, "remove deletes the member");
assert((await c.count({})) === 1, "size after remove");

console.log("ALL OK");
"#;

const CACHE_SOURCE: &str = "context shop\n\
\n\
capability Clock {\n\
\x20 fn now() -> Effect[Int]\n\
}\n\
\n\
provides Clock = SystemClock {\n\
\x20 fn now() -> Effect[Int] {\n\
\x20   0\n\
\x20 }\n\
}\n\
\n\
agent Sessions {\n\
\x20 key id: String\n\
\x20 store live: Cache[String, Int] @ttl(1.minutes)\n\
\n\
\x20 on call put(k: String, n: Int) -> Effect[()] given Clock {\n\
\x20   let _ <- live.put(k, n)\n\
\x20   Effect.pure(())\n\
\x20 }\n\
\x20 on call fetch(k: String) -> Effect[Option[Int]] given Clock {\n\
\x20   let r <- live.get(k)\n\
\x20   Effect.pure(r)\n\
\x20 }\n\
\x20 on call has(k: String) -> Effect[Bool] given Clock {\n\
\x20   let b <- live.contains(k)\n\
\x20   Effect.pure(b)\n\
\x20 }\n\
\x20 on call count() -> Effect[Int] given Clock {\n\
\x20   let n <- live.size()\n\
\x20   Effect.pure(n)\n\
\x20 }\n\
}\n";

const CACHE_DRIVER_TS: &str = r#"
import { Sessions } from "./shop.js";

function assert(cond: boolean, msg: string): void {
  if (!cond) {
    throw new Error(`assertion failed: ${msg}`);
  }
}

function fakeState() {
  const m = new Map<string, unknown>();
  return {
    storage: {
      async get(key: string): Promise<unknown> { return m.get(key); },
      async put(key: string, value: unknown): Promise<void> { m.set(key, value); },
    },
  };
}

// A controllable mock clock — the testability the `given Clock` design buys
// (ADR 0113 D4). `nowMs` is advanced by the driver to drive TTL expiry.
let nowMs = 1_000_000;
const clock = { now: async (): Promise<number> => nowMs };
const deps = { Clock: clock };

const c = new Sessions(fakeState() as never);

// put + get within the TTL window
await c.put("a", 5, deps);
let r = await c.fetch("a", deps);
assert(r.tag === "Some" && r.value === 5, "live entry reads back");
assert((await c.has("a", deps)) === true, "contains is true while live");
assert((await c.count(deps)) === 1, "size counts the live entry");

// advance past the 1-minute TTL — the entry expires (lazy, check-on-read)
nowMs += 60_001;
r = await c.fetch("a", deps);
assert(r.tag === "None", "an entry past its TTL reads as None");
assert((await c.has("a", deps)) === false, "contains is false once expired");
assert((await c.count(deps)) === 0, "size drops the expired entry");

// a fresh put resets the lifetime
await c.put("a", 9, deps);
r = await c.fetch("a", deps);
assert(r.tag === "Some" && r.value === 9, "put resets the entry's TTL");

console.log("ALL OK");
"#;

// v0.94 (ADR 0116/0120): joins & grouping in the combiner form. A storage
// `joinOn`/`leftJoin`/`groupBy` over two store maps (the lazy `Query` lowering),
// plus an in-memory `List` join (the eager lowering), projecting each result
// through `into` — there is no pair value.
const JOIN_SOURCE: &str = "context shop\n\
\n\
type Order = {\n\
\x20 id: String,\n\
\x20 customer: String,\n\
}\n\
\n\
type Line = {\n\
\x20 orderId: String,\n\
\x20 qty: Int,\n\
}\n\
\n\
type Joined = {\n\
\x20 customer: String,\n\
\x20 qty: Int,\n\
}\n\
\n\
type Tot = {\n\
\x20 orderId: String,\n\
\x20 total: Int,\n\
}\n\
\n\
agent Sales {\n\
\x20 key k: String\n\
\x20 store orders: Map[String, Order]\n\
\x20 store lines: Map[String, Line]\n\
\n\
\x20 on call addOrder(id: String, c: String) -> Effect[()] {\n\
\x20   let _ <- orders.put(id, Order { id: id, customer: c })\n\
\x20   Effect.pure(())\n\
\x20 }\n\
\x20 on call addLine(lid: String, oid: String, q: Int) -> Effect[()] {\n\
\x20   let _ <- lines.put(lid, Line { orderId: oid, qty: q })\n\
\x20   Effect.pure(())\n\
\x20 }\n\
\x20 on call innerCount() -> Effect[Int] {\n\
\x20   lines.joinOn(orders, (l) => l.orderId, (o) => o.id, (l, o) => Joined { customer: o.customer, qty: l.qty }).count()\n\
\x20 }\n\
\x20 on call innerQty() -> Effect[Int] {\n\
\x20   lines.joinOn(orders, (l) => l.orderId, (o) => o.id, (l, o) => Joined { customer: o.customer, qty: l.qty }).sum((j) => j.qty)\n\
\x20 }\n\
\x20 on call leftCount() -> Effect[Int] {\n\
\x20   lines.leftJoin(orders, (l) => l.orderId, (o) => o.id, (l, mo) => Joined { customer: \"x\", qty: l.qty }).count()\n\
\x20 }\n\
\x20 on call groupCount() -> Effect[Int] {\n\
\x20   lines.groupBy((l) => l.orderId, (oid, rows) => Tot { orderId: oid, total: rows.sum((r) => r.qty) }).count()\n\
\x20 }\n\
\x20 on call listJoin(os: List[Order], ls: List[Line]) -> Effect[Int] {\n\
\x20   Effect.pure(ls.joinOn(os, (l) => l.orderId, (o) => o.id, (l, o) => Joined { customer: o.customer, qty: l.qty }).length())\n\
\x20 }\n\
}\n";

const JOIN_DRIVER_TS: &str = r#"
import { Sales } from "./shop.js";

function assert(cond: boolean, msg: string): void {
  if (!cond) {
    throw new Error(`assertion failed: ${msg}`);
  }
}

function fakeState() {
  const m = new Map<string, unknown>();
  return {
    storage: {
      async get(key: string): Promise<unknown> { return m.get(key); },
      async put(key: string, value: unknown): Promise<void> { m.set(key, value); },
    },
  };
}

const c = new Sales(fakeState() as never);

// Two orders; four lines, one (l4) referencing a non-existent order o3.
await c.addOrder("o1", "alice", {});
await c.addOrder("o2", "bob", {});
await c.addLine("l1", "o1", 5, {});
await c.addLine("l2", "o1", 3, {});
await c.addLine("l3", "o2", 7, {});
await c.addLine("l4", "o3", 9, {});

// Inner equi-join: l1/l2/l3 match; l4 (o3) has no order → dropped.
assert((await c.innerCount({})) === 3, "joinOn keeps only matched rows");
assert((await c.innerQty({})) === 15, "joinOn projects through into (5+3+7)");

// Left join: every line survives, l4 with the unmatched (None) branch → 4.
assert((await c.leftCount({})) === 4, "leftJoin keeps unmatched left rows");

// groupBy: three distinct orderIds (o1, o2, o3) → three groups.
assert((await c.groupCount({})) === 3, "groupBy partitions by key");

// In-memory List join (eager lowering): same matching as the storage join.
const os = [{ id: "o1", customer: "alice" }, { id: "o2", customer: "bob" }];
const ls = [
  { orderId: "o1", qty: 5 }, { orderId: "o1", qty: 3 },
  { orderId: "o2", qty: 7 }, { orderId: "o3", qty: 9 },
];
assert((await c.listJoin(os, ls, {})) === 3, "in-memory joinOn matches like the storage join");

console.log("ALL OK");
"#;

// v0.93 (ADR 0118): a `store Map @indexed(by: orderId)` — the secondary index is
// maintained inside the commit (re-index on last-write-wins / update / remove)
// and an equality `filter` on the indexed field routes to a posting-list lookup.
const INDEX_SOURCE: &str = "context shop\n\
\n\
type Reservation = {\n\
\x20 id: String,\n\
\x20 orderId: String,\n\
\x20 qty: Int,\n\
}\n\
\n\
agent Inventory {\n\
\x20 key sku: String\n\
\x20 store reservations: Map[String, Reservation] @indexed(by: orderId)\n\
\n\
\x20 on call reserve(rid: String, oid: String, n: Int) -> Effect[()] {\n\
\x20   let _ <- reservations.put(rid, Reservation { id: rid, orderId: oid, qty: n })\n\
\x20   Effect.pure(())\n\
\x20 }\n\
\x20 on call retag(rid: String, oid: String) -> Effect[()] {\n\
\x20   let _ <- reservations.update(rid, (r) => Reservation { ...r, orderId: oid })\n\
\x20   Effect.pure(())\n\
\x20 }\n\
\x20 on call setQty(rid: String, n: Int) -> Effect[()] {\n\
\x20   let _ <- reservations.update(rid, (r) => Reservation { ...r, qty: n })\n\
\x20   Effect.pure(())\n\
\x20 }\n\
\x20 on call drop(rid: String) -> Effect[()] {\n\
\x20   let _ <- reservations.remove(rid)\n\
\x20   Effect.pure(())\n\
\x20 }\n\
\x20 on call countForOrder(oid: String) -> Effect[Int] {\n\
\x20   reservations.filter((r) => r.orderId == oid).count()\n\
\x20 }\n\
\x20 on call qtyForOrder(oid: String) -> Effect[Int] {\n\
\x20   reservations.filter((r) => r.orderId == oid).sum((r) => r.qty)\n\
\x20 }\n\
}\n";

const INDEX_DRIVER_TS: &str = r#"
import { Inventory } from "./shop.js";

function assert(cond: boolean, msg: string): void {
  if (!cond) {
    throw new Error(`assertion failed: ${msg}`);
  }
}

function fakeState() {
  const m = new Map<string, unknown>();
  return {
    storage: {
      async get(key: string): Promise<unknown> { return m.get(key); },
      async put(key: string, value: unknown): Promise<void> { m.set(key, value); },
    },
  };
}

const c = new Inventory(fakeState() as never);

// Two reservations on o1, one on o2.
await c.reserve("r1", "o1", 5, {});
await c.reserve("r2", "o1", 3, {});
await c.reserve("r3", "o2", 7, {});
assert((await c.countForOrder("o1", {})) === 2, "indexed filter returns both o1 rows");
assert((await c.qtyForOrder("o1", {})) === 8, "indexed filter sums o1 (5+3)");
assert((await c.countForOrder("o2", {})) === 1, "indexed filter returns the o2 row");
assert((await c.countForOrder("o3", {})) === 0, "an unseen key yields no rows");

// Re-tag r2 from o1 to o2 (update the indexed field) — postings must move.
await c.retag("r2", "o2", {});
assert((await c.countForOrder("o1", {})) === 1, "re-index drops the old posting");
assert((await c.countForOrder("o2", {})) === 2, "re-index adds the new posting");
assert((await c.qtyForOrder("o2", {})) === 10, "re-index keeps sums correct (7+3)");

// Last-write-wins: re-put r1 under a different order.
await c.reserve("r1", "o2", 5, {});
assert((await c.countForOrder("o1", {})) === 0, "last-write-wins drops the stale posting");
assert((await c.countForOrder("o2", {})) === 3, "last-write-wins adds the new posting");

// Remove drops the posting.
await c.drop("r3", {});
assert((await c.countForOrder("o2", {})) === 2, "remove drops the posting");

// A non-indexed update keeps the index but changes the value.
await c.setQty("r1", 100, {});
assert((await c.countForOrder("o2", {})) === 2, "a non-indexed update leaves postings intact");
assert((await c.qtyForOrder("o2", {})) === 103, "the updated value flows through (3+100)");

console.log("ALL OK");
"#;

const LOG_SOURCE: &str = "context shop\n\
\n\
capability Clock {\n\
\x20 fn now() -> Effect[Instant]\n\
}\n\
\n\
provides Clock = SystemClock {\n\
\x20 fn now() -> Effect[Instant] {\n\
\x20   Instant.fromEpochMillis(0)\n\
\x20 }\n\
}\n\
\n\
agent Audit {\n\
\x20 key id: String\n\
\x20 store events: Log[Int] @retain(1.minutes)\n\
\n\
\x20 on call write(n: Int) -> Effect[()] given Clock {\n\
\x20   let _ <- events.append(n)\n\
\x20   Effect.pure(())\n\
\x20 }\n\
\x20 on call dump() -> Effect[List[Int]] {\n\
\x20   events.collect()\n\
\x20 }\n\
\x20 on call countSince(t: Instant) -> Effect[Int] {\n\
\x20   events.since(t).count()\n\
\x20 }\n\
\x20 on call lastN(n: Int) -> Effect[List[Int]] {\n\
\x20   events.recent(n).collect()\n\
\x20 }\n\
}\n";

const LOG_DRIVER_TS: &str = r#"
import { Audit } from "./shop.js";

function assert(cond: boolean, msg: string): void {
  if (!cond) {
    throw new Error(`assertion failed: ${msg}`);
  }
}
function eq<T>(a: T, b: T, msg: string): void {
  assert(JSON.stringify(a) === JSON.stringify(b), `${msg} (got ${JSON.stringify(a)})`);
}

function fakeState() {
  const m = new Map<string, unknown>();
  return {
    storage: {
      async get(key: string): Promise<unknown> { return m.get(key); },
      async put(key: string, value: unknown): Promise<void> { m.set(key, value); },
    },
  };
}

// A controllable mock clock (the testability `given Clock` buys, ADR 0121 D2).
let nowMs = 1_000;
const clock = { now: async (): Promise<number> => nowMs };
const deps = { Clock: clock };

const c = new Audit(fakeState() as never);

// append stamps the clock; advance it between writes
await c.write(1, deps);                 // t=1000
nowMs = 2_000; await c.write(2, deps);  // t=2000
nowMs = 3_000; await c.write(3, deps);  // t=3000

// collect (clock-free read) yields the values in append order
eq(await c.dump({}), [1, 2, 3], "collect is append-ordered");

// since(Instant) filters by timestamp — a clock-free read (explicit bound)
assert((await c.countSince(2_000, {})) === 2, "since(2000) keeps t>=2000");
assert((await c.countSince(3_001, {})) === 0, "since after the last entry is empty");

// recent(n) is the last n, newest first
eq(await c.lastN(2, {}), [3, 2], "recent(2) is newest-first");

// @retain prunes on append: t=70000 drops everything older than 70000-60000
nowMs = 70_000; await c.write(4, deps);
eq(await c.dump({}), [4], "retention prunes entries past the window on append");

console.log("ALL OK");
"#;

const TSCONFIG_JSON: &str = r#"{
  "compilerOptions": {
    "module": "Node16",
    "moduleResolution": "node16",
    "target": "ES2022",
    "strict": true,
    "skipLibCheck": true,
    "outDir": "js",
    "rootDir": ".",
    "lib": ["ES2022", "DOM"]
  },
  "include": ["**/*.ts"],
  "exclude": ["js"]
}
"#;

/// Compile `source` (a one-file `context shop` project, bundle target), write it
/// alongside `driver`, then `tsc`-compile and run the driver under node, asserting
/// it prints `ALL OK`. Skips loudly without a TS toolchain.
fn verify(tag: &str, source: &str, driver: &str) {
    let runner = match discover_tsc() {
        Some(r) => r,
        None => {
            eprintln!("\n!!! STORE-BEHAVIOUR VERIFICATION SKIPPED !!!\nno `tsc`/`npx` on PATH.\n");
            if std::env::var(REQUIRE_ENV).is_ok() {
                panic!("{REQUIRE_ENV} is set but no tsc runner was found");
            }
            return;
        }
    };
    if !tool_exists("node") {
        eprintln!("\n!!! STORE-BEHAVIOUR VERIFICATION SKIPPED !!!\n`node` is not on PATH.\n");
        if std::env::var(REQUIRE_ENV).is_ok() {
            panic!("{REQUIRE_ENV} is set but `node` was not found");
        }
        return;
    }

    let tmp =
        std::env::temp_dir().join(format!("bynk-store-behaviour-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    let src = tmp.join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("shop.bynk"), source).unwrap();
    let out = bynkc::compile_project(&bynkc::CompileOptions::single(src.clone()))
        .map_err(bynkc::ProjectFailure::flatten)
        .expect("the store agent must compile");

    for f in &out.files {
        let p = f.output_path.to_string_lossy();
        if p == "tsconfig.json" {
            continue;
        }
        let target_path = tmp.join(&f.output_path);
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&target_path, &f.typescript).unwrap();
    }
    fs::write(tmp.join("driver.ts"), driver).unwrap();
    fs::write(tmp.join("tsconfig.json"), TSCONFIG_JSON).unwrap();
    fs::write(tmp.join("package.json"), "{ \"type\": \"module\" }").unwrap();

    let (program, prefix) = &runner;
    let (ok, out_text) = run(program, prefix, &["-p", "tsconfig.json"], &tmp);
    assert!(
        ok,
        "tsc failed on the store-agent driver ({tag}):\n{out_text}"
    );

    let (ok, out_text) = run("node", &[], &["js/driver.js"], &tmp);
    let _ = fs::remove_dir_all(&tmp);
    assert!(
        ok && out_text.contains("ALL OK"),
        "store-agent behaviour driver ({tag}) did not pass:\n{out_text}"
    );
}

#[test]
fn store_cell_agent_runtime_semantics() {
    verify("cell", SOURCE, DRIVER_TS);
}

#[test]
fn store_map_agent_runtime_semantics() {
    verify("map", MAP_SOURCE, MAP_DRIVER_TS);
}

#[test]
fn store_set_agent_runtime_semantics() {
    verify("set", SET_SOURCE, SET_DRIVER_TS);
}

#[test]
fn store_indexed_map_runtime_semantics() {
    verify("index", INDEX_SOURCE, INDEX_DRIVER_TS);
}

#[test]
fn query_join_and_group_runtime_semantics() {
    verify("join", JOIN_SOURCE, JOIN_DRIVER_TS);
}

#[test]
fn store_log_agent_runtime_semantics() {
    verify("log", LOG_SOURCE, LOG_DRIVER_TS);
}

#[test]
fn store_cache_agent_runtime_semantics() {
    verify("cache", CACHE_SOURCE, CACHE_DRIVER_TS);
}

// v0.96 (ADR 0124): the rehydration validation gate. A loaded refined field that
// fails its predicate — or a structurally-corrupt one — faults with a
// `RehydrationViolation` before any handler reads it (Q6); a `store` field absent
// from a record written before it existed takes its default (D4 additive
// evolution), without faulting.
const REHYDRATE_SOURCE: &str = "context shop\n\
\n\
type Pos = Int where Positive\n\
\n\
agent Gauge {\n\
\x20 key id: String\n\
\n\
\x20 store level: Cell[Pos] = 1\n\
\x20 store note:  Cell[String]\n\
\n\
\x20 on call read() -> Effect[Int] {\n\
\x20   level\n\
\x20 }\n\
}\n";

const REHYDRATE_DRIVER_TS: &str = r#"
import { Gauge } from "./shop.js";

function assert(cond: boolean, msg: string): void {
  if (!cond) {
    throw new Error(`assertion failed: ${msg}`);
  }
}

function fakeState() {
  const m = new Map<string, unknown>();
  return {
    storage: {
      async get(key: string): Promise<unknown> { return m.get(key); },
      async put(key: string, value: unknown): Promise<void> { m.set(key, value); },
    },
  };
}

// 1) A refined field that violates its predicate on load faults — and as a
//    `RehydrationViolation`, naming the agent, never the value.
const st1 = fakeState();
await st1.storage.put("state", { level: -5, note: "x" });
const g1 = new Gauge(st1 as never);
let threw1 = false;
try {
  await g1.read({});
} catch (e) {
  const rv = (e as { rehydrationViolation?: { kind: string; agent: string } }).rehydrationViolation;
  threw1 = rv?.kind === "RehydrationViolation" && rv?.agent === "Gauge";
}
assert(threw1, "a refined field failing on load throws RehydrationViolation");

// 2) A structurally-corrupt field (wrong type) faults too.
const st2 = fakeState();
await st2.storage.put("state", { level: "oops", note: "x" });
const g2 = new Gauge(st2 as never);
let threw2 = false;
try {
  await g2.read({});
} catch (e) {
  threw2 = (e as { rehydrationViolation?: { kind: string } }).rehydrationViolation?.kind === "RehydrationViolation";
}
assert(threw2, "a structurally-corrupt field throws RehydrationViolation");

// 3) Additive evolution: `note` is absent from a record written before the field
//    existed, so it takes its zero — no fault — and the valid `level` is returned.
const st3 = fakeState();
await st3.storage.put("state", { level: 2 });
const g3 = new Gauge(st3 as never);
assert((await g3.read({})) === 2, "an absent (additive) field defaults rather than faulting");

// 4) A fully-valid stored record rehydrates cleanly.
const st4 = fakeState();
await st4.storage.put("state", { level: 3, note: "ok" });
const g4 = new Gauge(st4 as never);
assert((await g4.read({})) === 3, "valid stored state rehydrates");

console.log("ALL OK");
"#;

#[test]
fn store_rehydration_gate_runtime_semantics() {
    verify("rehydration", REHYDRATE_SOURCE, REHYDRATE_DRIVER_TS);
}
