function __bynkExec(): { waitUntil(promise: Promise<unknown>): void; pending: Promise<unknown>[] } {
  const pending: Promise<unknown>[] = [];
  return { pending, waitUntil(promise: Promise<unknown>): void { pending.push(promise); } };
}
async function __bynkSettle<T>(deps: { __exec: { pending: Promise<unknown>[] } }, call: Promise<T>): Promise<T> {
  const result = await call;
  while (deps.__exec.pending.length > 0) {
    await Promise.allSettled(deps.__exec.pending.splice(0));
  }
  return result;
}
function __bynkFetchSettled<T>(fetch: (exec: { waitUntil(promise: Promise<unknown>): void }) => Promise<T>): Promise<T> {
  const __exec = __bynkExec();
  return __bynkSettle({ __exec }, fetch(__exec));
}
