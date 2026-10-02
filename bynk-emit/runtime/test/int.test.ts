import { test } from "node:test";
import assert from "node:assert/strict";
import { __bynkIntDiv, __bynkToInt } from "../src/int.ts";

test("#1657 __bynkIntDiv truncates toward zero and traps on zero", () => {
  assert.equal(__bynkIntDiv(7, 2), 3);
  assert.equal(__bynkIntDiv(-7, 2), -3);
  assert.throws(() => __bynkIntDiv(5, 0), /Int division by zero/);
  assert.throws(() => __bynkIntDiv(0, 0), /Int division by zero/);
});

test("#1657 __bynkToInt passes a safe integer and traps otherwise", () => {
  assert.equal(__bynkToInt(Math.round(2.5), "round"), 3);
  assert.throws(() => __bynkToInt(Math.round(Infinity), "round"), /Float.round: result is not a safe Int/);
  assert.throws(() => __bynkToInt(Math.floor(NaN), "floor"), /not a safe Int/);
  assert.throws(() => __bynkToInt(Math.trunc(1e300), "truncate"), /not a safe Int/);
});
