// Semantic-debugging slice 1 (ADR 0105): render Bynk's tagged ADT values in the
// debugger in Bynk's vocabulary — `Ok(42)`, not `{tag: "Ok", value: 42}` — by
// rewriting js-debug's response *previews* in the extension host (so it is
// runtime-agnostic and reaches workerd, unlike slice 5's in-debuggee generator).
//
// The rewrite is synchronous (it runs while the DAP response is delivered), so it
// works from the preview string js-debug already produced — e.g.
// `{tag: 'Some', value: 'hi'}` — not an async fetch of the object's fields. This
// module is the pure renderer: it parses that preview with a real recursive parser
// (so braces inside strings don't fool it) and re-renders a *tagged* object as Bynk
// constructor syntax. It is **total** — any non-tagged value, or anything it can't
// parse cleanly, comes back byte-for-byte unchanged.

type PreviewValue =
  | { kind: "obj"; entries: [string, PreviewValue][]; truncated: boolean }
  | { kind: "arr"; items: PreviewValue[]; truncated: boolean }
  | { kind: "str"; value: string }
  | { kind: "raw"; text: string };

/** Render js-debug's preview of a value into Bynk constructor syntax when it is a
 *  tagged ADT (`{tag: '…', …}`); otherwise return the preview unchanged. Never
 *  throws. */
export function renderBynkValue(preview: string): string {
  const s = preview.trim();
  // Fast reject: only an object preview can be a tagged ADT.
  if (s.length === 0 || s[0] !== "{") return preview;
  try {
    const p = new PreviewParser(s);
    const v = p.parse();
    p.skipWs();
    if (!p.atEnd()) return preview; // trailing junk → don't trust the parse
    // Only rewrite a *tagged* top-level object; leave plain objects/values alone.
    if (v.kind === "obj" && tagOf(v) !== undefined) return render(v);
    return preview;
  } catch {
    return preview;
  }
}

function tagOf(v: Extract<PreviewValue, { kind: "obj" }>): string | undefined {
  const tag = v.entries.find(([k]) => k === "tag");
  return tag && tag[1].kind === "str" ? tag[1].value : undefined;
}

function render(v: PreviewValue): string {
  switch (v.kind) {
    case "str":
      return JSON.stringify(v.value); // Bynk-style double-quoted, properly escaped
    case "raw":
      return v.text; // numbers, booleans, null, idents, the `…` truncation marker
    case "arr": {
      const items = v.items.map(render);
      if (v.truncated) items.push("…");
      return "[" + items.join(", ") + "]";
    }
    case "obj": {
      const tag = tagOf(v);
      if (tag !== undefined) {
        const fields = v.entries.filter(([k]) => k !== "tag").map(([, val]) => render(val));
        if (v.truncated) fields.push("…");
        return fields.length ? `${tag}(${fields.join(", ")})` : tag;
      }
      // Plain object — reconstruct it (so a plain field of a tagged value still reads).
      const parts = v.entries.map(([k, val]) => `${k}: ${render(val)}`);
      if (v.truncated) parts.push("…");
      return "{" + parts.join(", ") + "}";
    }
  }
}

/** A small recursive-descent parser for js-debug's value-preview display grammar:
 *  objects `{k: v, …}`, arrays `[v, …]`, single-quoted strings, and raw tokens
 *  (numbers/idents/`…`). Strings are scanned with escape handling, so braces or
 *  commas *inside* a string never confuse structure. */
class PreviewParser {
  private i = 0;
  private readonly s: string;
  constructor(s: string) {
    this.s = s;
  }

  atEnd(): boolean {
    return this.i >= this.s.length;
  }
  skipWs(): void {
    while (this.i < this.s.length && /\s/.test(this.s[this.i])) this.i++;
  }
  private peek(): string {
    return this.s[this.i];
  }

  parse(): PreviewValue {
    this.skipWs();
    const c = this.peek();
    if (c === "{") return this.parseObject();
    if (c === "[") return this.parseArray();
    if (c === "'" || c === '"') return this.parseString();
    return this.parseRaw();
  }

  private parseObject(): PreviewValue {
    this.i++; // '{'
    const entries: [string, PreviewValue][] = [];
    let truncated = false;
    for (;;) {
      this.skipWs();
      if (this.atEnd()) throw new Error("unterminated object");
      const c = this.peek();
      if (c === "}") {
        this.i++;
        break;
      }
      if (this.isTruncation()) {
        truncated = true;
        this.consumeTruncation();
        continue;
      }
      if (c === ",") {
        this.i++;
        continue;
      }
      const key = this.parseKey();
      this.skipWs();
      if (this.peek() !== ":") throw new Error("expected ':'");
      this.i++;
      const value = this.parse();
      entries.push([key, value]);
    }
    return { kind: "obj", entries, truncated };
  }

  private parseArray(): PreviewValue {
    this.i++; // '['
    const items: PreviewValue[] = [];
    let truncated = false;
    for (;;) {
      this.skipWs();
      if (this.atEnd()) throw new Error("unterminated array");
      const c = this.peek();
      if (c === "]") {
        this.i++;
        break;
      }
      if (this.isTruncation()) {
        truncated = true;
        this.consumeTruncation();
        continue;
      }
      if (c === ",") {
        this.i++;
        continue;
      }
      items.push(this.parse());
    }
    return { kind: "arr", items, truncated };
  }

  private parseString(): PreviewValue {
    const quote = this.s[this.i];
    this.i++; // opening quote
    let value = "";
    while (this.i < this.s.length) {
      const c = this.s[this.i++];
      if (c === "\\") {
        const n = this.s[this.i++];
        // Keep common escapes legible; pass others through.
        value += n === "n" ? "\n" : n === "t" ? "\t" : n ?? "";
        continue;
      }
      if (c === quote) return { kind: "str", value };
      value += c;
    }
    throw new Error("unterminated string");
  }

  // A key is a bare identifier or a quoted string (js-debug uses bare for normal
  // property names; some keys arrive quoted).
  private parseKey(): string {
    const c = this.peek();
    if (c === "'" || c === '"') {
      const sv = this.parseString();
      return sv.kind === "str" ? sv.value : "";
    }
    let k = "";
    while (this.i < this.s.length && /[A-Za-z0-9_$]/.test(this.s[this.i])) k += this.s[this.i++];
    if (k.length === 0) throw new Error("expected key");
    return k;
  }

  // A raw token: a number/boolean/null/identifier — read until a structural char.
  private parseRaw(): PreviewValue {
    let t = "";
    while (this.i < this.s.length && !",}]:".includes(this.s[this.i])) {
      // stop a raw token at a string/object/array start too
      const c = this.s[this.i];
      if (c === "'" || c === '"' || c === "{" || c === "[") break;
      t += c;
      this.i++;
    }
    t = t.trim();
    if (t.length === 0) throw new Error("empty raw token");
    return { kind: "raw", text: t };
  }

  // js-debug renders an omitted/truncated remainder as `…` (U+2026), and sometimes
  // `...`. Treat either as a truncation marker.
  private isTruncation(): boolean {
    return this.s[this.i] === "…" || this.s.startsWith("...", this.i);
  }
  private consumeTruncation(): void {
    if (this.s[this.i] === "…") this.i++;
    else this.i += 3;
  }
}
