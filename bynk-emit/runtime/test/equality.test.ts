// #1652: structural `==` (runtime-semantics track §3.1).
import { test } from "node:test";
import assert from "node:assert/strict";
import { Some, None, Ok, Err } from "../src/result.ts";
import { __bynkEq } from "../src/equality.ts";

test("__bynkEq: records, sums, Option and Result compare structurally", () => {
  assert.ok(__bynkEq({ x: 1, y: 2 }, { x: 1, y: 2 }));
  assert.ok(!__bynkEq({ x: 1, y: 2 }, { x: 1, y: 3 }));
  assert.ok(__bynkEq(Some(1), Some(1)));
  assert.ok(!__bynkEq(Some(1), Some(2)));
  assert.ok(!__bynkEq(Some(1), None));
  assert.ok(__bynkEq(None, { tag: "None" }));
  assert.ok(__bynkEq(Ok({ a: [1, 2] }), Ok({ a: [1, 2] })));
  assert.ok(!__bynkEq(Ok(1), Err(1)));
  // A nullary variant decoded from JSON is a fresh object, not the singleton.
  const On = { tag: "On" };
  assert.ok(__bynkEq(On, JSON.parse('{"tag":"On"}')));
  assert.ok(__bynkEq({ tag: "Circle", r: 1 }, { tag: "Circle", r: 1 }));
  assert.ok(!__bynkEq({ tag: "Circle", r: 1 }, { tag: "Square", r: 1 }));
});

test("__bynkEq: List, Map and Bytes compare by content", () => {
  assert.ok(__bynkEq([1, 2], [1, 2]));
  assert.ok(!__bynkEq([1, 2], [1, 2, 3]));
  assert.ok(__bynkEq([[1], [2]], [[1], [2]]));
  assert.ok(__bynkEq(new Map([["a", Some(1)]]), new Map([["a", Some(1)]])));
  assert.ok(!__bynkEq(new Map([["a", 1]]), new Map([["b", 1]])));
  assert.ok(!__bynkEq(new Map([["a", 1]]), new Map([["a", 2]])));
  assert.ok(__bynkEq(new Uint8Array([1, 2]), new Uint8Array([1, 2])));
  assert.ok(!__bynkEq(new Uint8Array([1, 2]), new Uint8Array([2, 1])));
});

test("__bynkEq: Float keeps IEEE semantics at any depth", () => {
  assert.ok(!__bynkEq(NaN, NaN));
  assert.ok(!__bynkEq({ f: NaN }, { f: NaN }));
  assert.ok(__bynkEq(-0, 0));
  assert.ok(__bynkEq({ f: -0 }, { f: 0 }));
});

test("__bynkEq: different shapes and class instances are not equal", () => {
  assert.ok(!__bynkEq({ a: 1 }, { a: 1, b: 2 }));
  assert.ok(!__bynkEq([1], { 0: 1 }));
  class Handle {}
  const h = new Handle();
  assert.ok(__bynkEq(h, h));
  assert.ok(!__bynkEq(new Handle(), new Handle()));
  assert.ok(__bynkEq(undefined, undefined));
});
