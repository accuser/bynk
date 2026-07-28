function __bynkDeepEqual(a: unknown, b: unknown): boolean {
  const s = (v: unknown) => JSON.stringify(v, (_k, val) => typeof val === "bigint" ? "__bigint__" + String(val) : val);
  try { return s(a) === s(b); } catch { return a === b; }
}
