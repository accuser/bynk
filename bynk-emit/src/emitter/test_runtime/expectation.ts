class ExpectationError extends Error {
  location: string;
  start: number;
  end: number;
  constructor(location: string, start: number, end: number, detail: string) {
    super(`${detail}\n  at ${location}`);
    this.location = location;
    this.start = start;
    this.end = end;
  }
}
function __bynkExpectFailure(location: string, start: number, end: number, detail: string) {
  return new ExpectationError(location, start, end, detail);
}
function __bynkExpect(cond: boolean, location: string, start: number, end: number, detail: string): void {
  if (!cond) { throw __bynkExpectFailure(location, start, end, detail); }
}
function __bynkShow(v: unknown): string {
  try { return typeof v === "bigint" ? String(v) : (JSON.stringify(v) ?? String(v)); } catch { return String(v); }
}

