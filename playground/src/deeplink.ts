// The shareable snippet / deep-link format (in-browser track Q7; the contract is
// shared with the documentation track, which emits exactly these links).
//
// The source is compressed and carried in the URL **fragment** (`#…`), so a link
// never hits a server — fitting the fully-static posture. Format (ADR 0140):
//
//   #<base64url( deflate-raw( utf8(source) ) )>
//
// using the browser-native Compression Streams API, so there is no library
// dependency on either side of the contract. `DecompressionStream` reverses it.

async function pipeThrough(bytes: Uint8Array, stream: GenericTransformStream): Promise<Uint8Array> {
  const writer = (stream.writable as WritableStream<Uint8Array>).getWriter();
  void writer.write(bytes);
  void writer.close();
  const buf = await new Response(stream.readable as ReadableStream<Uint8Array>).arrayBuffer();
  return new Uint8Array(buf);
}

function toBase64Url(bytes: Uint8Array): string {
  let bin = "";
  for (const b of bytes) bin += String.fromCharCode(b);
  return btoa(bin).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function fromBase64Url(s: string): Uint8Array {
  const b64 = s.replace(/-/g, "+").replace(/_/g, "/");
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

/// Encode source into a URL fragment (without the leading `#`).
export async function encodeSnippet(source: string): Promise<string> {
  const utf8 = new TextEncoder().encode(source);
  const deflated = await pipeThrough(utf8, new CompressionStream("deflate-raw"));
  return toBase64Url(deflated);
}

/// Decode a URL fragment back to source. Returns `null` for an empty/garbled hash.
export async function decodeSnippet(fragment: string): Promise<string | null> {
  const hash = fragment.replace(/^#/, "").trim();
  if (!hash) return null;
  try {
    const deflated = fromBase64Url(hash);
    const utf8 = await pipeThrough(deflated, new DecompressionStream("deflate-raw"));
    return new TextDecoder().decode(utf8);
  } catch {
    return null;
  }
}
