import { test } from "node:test";
import assert from "node:assert/strict";
import {
  __bynkBytesEqual,
  __bynkBytesToBase64,
  __bynkBytesFromBase64,
  __bynkBytesDecodeUtf8,
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
