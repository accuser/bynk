export interface DurableObjectStorage {
  get<T>(key: string): Promise<T | undefined>;
  put(key: string, value: unknown): Promise<void>;
  delete(key: string): Promise<boolean>;
  list<T>(options?: { prefix?: string }): Promise<Map<string, T>>;
}

export interface DurableObjectState {
  readonly storage: DurableObjectStorage;
  readonly id: { readonly name: string };
}

/**
 * Copy a stored value the way a Durable Object's storage round-trip does, for
 * every value that is **data**. #1660 (runtime-semantics track §3.5).
 *
 * - **Copied deeply:** plain objects, arrays, `Map`, `Uint8Array`, `Date`.
 * - **Passed through by reference:** an instance of any other class.
 *
 * The only such instances agent state holds are live handles. A held
 * `Connection` on the bundle target is a `TestConnection` stored directly in
 * state. On workers, a held connection persists its `connId` instead, and
 * `resolveConnection` re-presents the **same** live socket on load, so sharing
 * the handle here is the faithful analogue. `structuredClone` would instead
 * strip the handle's methods, and `send` would vanish.
 */
function cloneStored<T>(value: T): T {
  if (value === null || typeof value !== "object") return value;
  // `new Uint8Array(v)` copies; `slice()` on a Node `Buffer` (a Uint8Array) is a view.
  if (value instanceof Uint8Array) return new Uint8Array(value) as T;
  if (value instanceof Date) return new Date(value.getTime()) as T;
  if (value instanceof Map) {
    return new Map([...value].map(([k, v]) => [cloneStored(k), cloneStored(v)])) as T;
  }
  if (Array.isArray(value)) return value.map(cloneStored) as T;
  const proto = Object.getPrototypeOf(value);
  if (proto !== Object.prototype && proto !== null) return value;
  // `Object.fromEntries` *defines* each key, where `out[k] = v` would *set* it.
  // For an own `__proto__` key (which `JSON.parse` creates, so a decoded record
  // stored as a `Map[String, _]` can carry one), a set would reassign the copy's
  // prototype, or silently drop a primitive, instead of copying the entry.
  return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, cloneStored(v)])) as T;
}

/**
 * The bundle-target (and `bynkc test`) stand-in for a Durable Object's storage.
 *
 * #1660 (runtime-semantics track §3.5): every value is copied (`cloneStored`) on
 * the way in **and** on the way out, as workerd's storage copies it. A value read
 * back is never the object that was written. Two things depend on this.
 *
 * - **Fidelity.** Before #1660 this class handed back the stored reference. That
 *   let `bynkc test` pass programs that cannot work under workerd. A loaded enum
 *   compared `===` against its singleton constant (so reference `==`, #1652, was
 *   masked), and a handler's write through a shallow copy of loaded state was
 *   visible to the next load.
 * - **Atomicity (ADR 0109).** A handler begins from
 *   `{ ...(await this.loadState()) }`, which is a shallow copy. With a shared
 *   reference, a store write such as `__state.items[k] = v` mutated the record
 *   still held here, so a commit later *refused* by an invariant had already
 *   happened. Cloning on `get` gives each handler its own state, so a refused
 *   commit leaves storage untouched. The regression is pinned by
 *   `bynkc/tests/store_behaviour.rs`'s
 *   `refused_commit_does_not_leak_an_in_place_store_write`.
 */
export class InMemoryStorage implements DurableObjectStorage {
  private data = new Map<string, unknown>();

  async get<T>(key: string): Promise<T | undefined> {
    const value = this.data.get(key);
    return (value === undefined ? undefined : cloneStored(value)) as T | undefined;
  }

  async put(key: string, value: unknown): Promise<void> {
    this.data.set(key, cloneStored(value));
  }

  async delete(key: string): Promise<boolean> {
    return this.data.delete(key);
  }

  async list<T>(options?: { prefix?: string }): Promise<Map<string, T>> {
    const prefix = options?.prefix ?? "";
    const out = new Map<string, T>();
    for (const [k, v] of this.data) {
      if (k.startsWith(prefix)) out.set(k, cloneStored(v) as T);
    }
    return out;
  }
}

export function makeTestState(name: string): DurableObjectState {
  return {
    storage: new InMemoryStorage(),
    id: { name },
  };
}

export interface KVNamespace {
  get(key: string): Promise<string | null>;
  put(key: string, value: string, options?: { expirationTtl?: number }): Promise<void>;
  delete(key: string): Promise<void>;
  // v0.23: the page shape WorkersKv's drain consumes (0050).
  list(options?: { prefix?: string; cursor?: string }): Promise<{
    keys: { name: string }[];
    list_complete: boolean;
    cursor?: string;
  }>;
}
