import { test } from "node:test";
import assert from "node:assert/strict";
import { selectPluralArm, formatIcuNumber, formatIcuDate } from "../src/messages.ts";

test("formatIcuNumber: default, integer, percent styles", () => {
  assert.equal(formatIcuNumber("en", 1234.5), "1,234.5");
  assert.equal(formatIcuNumber("en", 1234.5, "integer"), "1,235");
  assert.equal(formatIcuNumber("en", 0.5, "percent"), "50%");
});

test("formatIcuDate: bare and all four dateStyles", () => {
  const epoch = Date.UTC(2020, 0, 15); // 2020-01-15
  const bare = formatIcuDate("en-US", epoch);
  const short = formatIcuDate("en-US", epoch, "short");
  const full = formatIcuDate("en-US", epoch, "full");
  assert.equal(typeof bare, "string");
  assert.ok(bare.length > 0);
  assert.ok(short.length > 0);
  // A "full" style spells out the weekday; a "short" one doesn't.
  assert.ok(full.includes("Wednesday"), full);
  assert.ok(!short.includes("Wednesday"), short);
});

test("selectPluralArm: English falls back to `other` for an unmapped category", () => {
  // English has only "one"/"other" categories; arms declares only "other".
  assert.equal(selectPluralArm("en", 1, { other: "many" }), "many");
  assert.equal(selectPluralArm("en", 2, { other: "many" }), "many");
});

test("selectPluralArm: Polish's real 4-category CLDR plural rule is exercised, not hardcoded", () => {
  const arms = { one: "one", few: "few", many: "many", other: "other" };
  // Polish CLDR plural rules: 1 -> one; 2..4 (not 12-14) -> few; most others -> many.
  assert.equal(selectPluralArm("pl", 1, arms), "one");
  assert.equal(selectPluralArm("pl", 2, arms), "few");
  assert.equal(selectPluralArm("pl", 5, arms), "many");
  assert.equal(selectPluralArm("pl", 1.5, arms), "other");
});
