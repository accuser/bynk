import { test } from "node:test";
import assert from "node:assert/strict";
import {
  __bynkBytesEqual,
  __bynkBytesToBase64,
  __bynkBytesFromBase64,
  __bynkBytesDecodeUtf8,
  __bynkBytesConcat,
  __bynkBytesCompare,
  __bynkBytesToHex,
  __bynkBytesFromHex,
  __bynkBytesToBase64Url,
  __bynkBytesFromBase64Url,
} from "../src/bytes.ts";

test("__bynkBytesEqual: content equality, not reference", () => {
  const a = new TextEncoder().encode("hello");
  const b = new TextEncoder().encode("hello");
  assert.notEqual(a, b); // distinct objects
  assert.equal(__bynkBytesEqual(a, b), true);
  assert.equal(__bynkBytesEqual(a, a), true);
  assert.equal(__bynkBytesEqual(a, new TextEncoder().encode("hellO")), false);
  assert.equal(__bynkBytesEqual(a, new TextEncoder().encode("hell")), false);
  assert.equal(__bynkBytesEqual(new Uint8Array(), new Uint8Array()), true);
});

test("base64 round-trips, empty included", () => {
  for (const s of ["", "f", "fo", "foo", "foob", "fooba", "foobar"]) {
    const bytes = new TextEncoder().encode(s);
    const b64 = __bynkBytesToBase64(bytes);
    const back = __bynkBytesFromBase64(b64);
    assert.equal(back.tag, "Some");
    if (back.tag === "Some") assert.equal(__bynkBytesEqual(back.value, bytes), true);
  }
  assert.equal(__bynkBytesToBase64(new Uint8Array()), "");
  assert.equal(__bynkBytesToBase64(new TextEncoder().encode("foobar")), "Zm9vYmFy");
});

test("__bynkBytesFromBase64: None on malformed input", () => {
  for (const bad of ["A", "AB=C", "!!!!", "Zm9vYmFy=", "====", "abc"]) {
    assert.equal(__bynkBytesFromBase64(bad).tag, "None", `expected None for ${JSON.stringify(bad)}`);
  }
});

test("__bynkBytesDecodeUtf8: Some for valid, None for invalid", () => {
  assert.deepEqual(__bynkBytesDecodeUtf8(new TextEncoder().encode("héllo")), {
    tag: "Some",
    value: "héllo",
  });
  // 0xFF is not a valid standalone UTF-8 byte.
  assert.equal(__bynkBytesDecodeUtf8(new Uint8Array([0xff])).tag, "None");
  assert.deepEqual(__bynkBytesDecodeUtf8(new Uint8Array()), { tag: "Some", value: "" });
});

// #2016 (standard-library K1): the `Bytes` operations.

const utf8 = (s: string) => new TextEncoder().encode(s);
const some = (o: { tag: string; value?: Uint8Array }): Uint8Array => {
  assert.equal(o.tag, "Some");
  return (o as { value: Uint8Array }).value;
};
const randomBuffers = (): Uint8Array[] => {
  const out: Uint8Array[] = [];
  for (let len = 0; len < 40; len++) {
    const b = new Uint8Array(len);
    for (let i = 0; i < len; i++) b[i] = (len * 131 + i * 197 + 7) & 0xff;
    out.push(b);
  }
  return out;
};

test("__bynkBytesConcat: joins without aliasing", () => {
  const a = utf8("foo");
  const b = utf8("bar");
  const c = __bynkBytesConcat(a, b);
  assert.equal(__bynkBytesEqual(c, utf8("foobar")), true);
  assert.notEqual(c, a);
  assert.equal(__bynkBytesConcat(new Uint8Array(), new Uint8Array()).length, 0);
  const e = new Uint8Array();
  assert.notEqual(__bynkBytesConcat(a, e), a, "a fresh buffer even when one side is empty");
});

test("__bynkBytesCompare: unsigned lexicographic, prefix first", () => {
  const u = (...xs: number[]) => new Uint8Array(xs);
  assert.equal(__bynkBytesCompare(u(), u()), 0);
  assert.equal(__bynkBytesCompare(u(1, 2), u(1, 2)), 0);
  assert.equal(__bynkBytesCompare(u(1), u(1, 0)), -1);
  assert.equal(__bynkBytesCompare(u(1, 0), u(1)), 1);
  assert.equal(__bynkBytesCompare(u(0x80), u(0x7f)), 1, "unsigned: 0x80 sorts after 0x7f");
  assert.equal(__bynkBytesCompare(u(0x7f), u(0x80)), -1);
  assert.equal(__bynkBytesCompare(u(2), u(1, 9)), 1, "the first difference decides, not length");
});

test("__bynkBytesToHex / FromHex: lowercase out, either case in", () => {
  assert.equal(__bynkBytesToHex(new Uint8Array([0x00, 0x0f, 0xab, 0xff])), "000fabff");
  assert.equal(__bynkBytesToHex(new Uint8Array()), "");
  assert.equal(__bynkBytesEqual(some(__bynkBytesFromHex("9F86d0")), new Uint8Array([0x9f, 0x86, 0xd0])), true);
  assert.equal(some(__bynkBytesFromHex("")).length, 0);
  for (const bad of ["0", "abc", "0x00", "sha256=00", "00 11", "00:11", "zz", " 00", "00\n"]) {
    assert.equal(__bynkBytesFromHex(bad).tag, "None", `expected None for ${JSON.stringify(bad)}`);
  }
  for (const b of randomBuffers()) {
    assert.equal(__bynkBytesEqual(some(__bynkBytesFromHex(__bynkBytesToHex(b))), b), true);
  }
});

test("__bynkBytesToBase64Url: RFC 4648 vectors, unpadded", () => {
  const vectors: [string, string][] = [
    ["", ""],
    ["f", "Zg"],
    ["fo", "Zm8"],
    ["foo", "Zm9v"],
    ["foob", "Zm9vYg"],
    ["fooba", "Zm9vYmE"],
    ["foobar", "Zm9vYmFy"],
  ];
  for (const [plain, enc] of vectors) {
    assert.equal(__bynkBytesToBase64Url(utf8(plain)), enc);
  }
  // The URL-safe alphabet: 0xfb 0xff encodes to "+/8=" in standard base64.
  assert.equal(__bynkBytesToBase64Url(new Uint8Array([0xfb, 0xff])), "-_8");
});

test("__bynkBytesFromBase64Url: padding optional, alphabets disjoint, canonical only", () => {
  assert.equal(__bynkBytesEqual(some(__bynkBytesFromBase64Url("Zm8")), utf8("fo")), true);
  assert.equal(__bynkBytesEqual(some(__bynkBytesFromBase64Url("Zm8=")), utf8("fo")), true);
  assert.equal(__bynkBytesEqual(some(__bynkBytesFromBase64Url("Zg==")), utf8("f")), true);
  assert.equal(__bynkBytesEqual(some(__bynkBytesFromBase64Url("-_8")), new Uint8Array([0xfb, 0xff])), true);
  assert.equal(some(__bynkBytesFromBase64Url("")).length, 0);
  for (const bad of ["+/8", "+/8=", "Zm9v=", "Zm9v==", "Zg=", "Zg===", "Z", "Zm9vY", "Zm 8", "=", "Zm=8", "Zh", "Zh==", "Zm9", "Zm9="]) {
    assert.equal(__bynkBytesFromBase64Url(bad).tag, "None", `expected None for ${JSON.stringify(bad)}`);
  }
  for (const b of randomBuffers()) {
    assert.equal(__bynkBytesEqual(some(__bynkBytesFromBase64Url(__bynkBytesToBase64Url(b))), b), true);
  }
});
