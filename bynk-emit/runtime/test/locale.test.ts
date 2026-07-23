import { test } from "node:test";
import assert from "node:assert/strict";
import { negotiateLocale } from "../src/locale.ts";

test("no header falls back to the reference locale", () => {
  assert.equal(negotiateLocale(undefined, ["en", "pt"], "en"), "en");
  assert.equal(negotiateLocale(null, ["en", "pt"], "en"), "en");
  assert.equal(negotiateLocale("", ["en", "pt"], "en"), "en");
});

test("exact match wins", () => {
  assert.equal(negotiateLocale("pt-BR", ["en", "pt-BR"], "en"), "pt-BR");
});

test("RFC 4647 basic filtering: pt-BR truncates to pt", () => {
  assert.equal(negotiateLocale("pt-BR", ["en", "pt"], "en"), "pt");
});

test("q-values reorder preference", () => {
  assert.equal(negotiateLocale("fr;q=0.5, pt;q=0.9", ["en", "pt"], "en"), "pt");
});

test("wildcard range is ignored", () => {
  assert.equal(negotiateLocale("*, pt", ["en", "pt"], "en"), "pt");
});

test("no declared locale matches any range: falls back to reference", () => {
  assert.equal(negotiateLocale("de-DE, ja", ["en", "pt"], "en"), "en");
});

test("case-insensitive match", () => {
  assert.equal(negotiateLocale("PT-br", ["en", "pt"], "en"), "pt");
});

test("multi-range Accept-Language: pt-BR,en;q=0.5 picks pt over the lower-weighted exact en", () => {
  assert.equal(negotiateLocale("pt-BR,en;q=0.5", ["en", "pt"], "en"), "pt");
});

test("malformed q-value is ignored, defaulting to q=1", () => {
  assert.equal(negotiateLocale("pt;q=bogus", ["en", "pt"], "en"), "pt");
});
