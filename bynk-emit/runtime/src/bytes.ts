import type { Option } from "./result.ts";
import { Some, None } from "./result.ts";

// v0.110 (ADR 0142): the `Bytes` primitive runtime. A `Bytes` erases to a
// `Uint8Array` (an immutable finite octet sequence at the Bynk level), so
// unlike the number-erased base types its equality is by content, not host
// `===`, and its wire form is a base64 string. These helpers back the kernel
// (`==`, `toBase64`, `decodeUtf8`) and the JSON codec (`fromBase64`).

// Content equality — the byte-for-byte compare the emitter lowers `==` to. The
// `===` fast path catches the identity case; otherwise length then each octet.
export function __bynkBytesEqual(a: Uint8Array, b: Uint8Array): boolean {
  if (a === b) return true;
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) {
    if (a[i] !== b[i]) return false;
  }
  return true;
}

// Total encode: the standard base64 of the octet sequence. Every byte is
// 0..255, so the Latin-1 string handed to `btoa` never overflows.
export function __bynkBytesToBase64(bytes: Uint8Array): string {
  let bin = "";
  for (let i = 0; i < bytes.length; i++) {
    bin += String.fromCharCode(bytes[i]);
  }
  return btoa(bin);
}

// Partial decode: `Some(bytes)` for a valid standard-base64 string, `None`
// otherwise. The alphabet/padding are validated up front so a malformed string
// is a clean `None`, not a silently truncated buffer (some `atob`
// implementations are lenient). The empty string decodes to empty bytes — the
// round-trip of `Bytes.empty`.
export function __bynkBytesFromBase64(s: string): Option<Uint8Array> {
  if (s.length % 4 !== 0 || !/^[A-Za-z0-9+/]*={0,2}$/.test(s)) {
    return None;
  }
  let bin: string;
  try {
    bin = atob(s);
  } catch {
    return None;
  }
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) {
    out[i] = bin.charCodeAt(i);
  }
  return Some(out);
}

// Partial decode to text: `Some(string)` when the octets are valid UTF-8,
// `None` otherwise. `fatal: true` makes an invalid sequence throw rather than
// emit replacement characters, so the partiality is real and surfaced.
export function __bynkBytesDecodeUtf8(bytes: Uint8Array): Option<string> {
  try {
    return Some(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
  } catch {
    return None;
  }
}
