// Unit coverage for the preview parser (src/semanticValues.ts) — the pure renderer
// the interposer applies. The end-to-end debug tests cover the common shapes; this
// pins the *adversarial* cases they can't easily produce: braces/commas/tag-like
// content inside strings (the reason a real parser beats a regex), truncation, and
// the totality guarantee (any non-tagged value comes back byte-for-byte).

import * as assert from "assert";

import { renderBynkValue } from "../../src/semanticValues";

describe("renderBynkValue (preview parser)", () => {
  const cases: [string, string][] = [
    // Common shapes
    ["{tag: 'Ok', value: 42}", "Ok(42)"],
    ["{tag: 'None'}", "None"],
    ["{tag: 'Some', value: 'hi'}", 'Some("hi")'],
    ["{tag: 'BadRequest', message: 'oops'}", 'BadRequest("oops")'],
    ["{tag: 'Ok', value: {tag: 'Some', value: 42}}", "Ok(Some(42))"],
    ["{tag: 'Created', value: {id: 7, name: 'x'}}", 'Created({id: 7, name: "x"})'],
    ["{tag: 'List', items: [1, 2, 3]}", "List([1, 2, 3])"],
    // Truncation (js-debug elides deep/long previews) is preserved
    ["{tag: 'Ok', value: {…}}", "Ok({…})"],
    ["{tag: 'Big', xs: [1, 2, …]}", "Big([1, 2, …])"],
    // Adversarial: structure characters INSIDE a string must not fool the parser
    ["{tag: 'Weird', message: 'has } brace and , comma'}", 'Weird("has } brace and , comma")'],
    ["{tag: 'X', s: '{tag: fake}'}", 'X("{tag: fake}")'],
    // Totality: non-tagged values returned unchanged
    ["{id: 7, name: 'x'}", "{id: 7, name: 'x'}"],
    ["{value: 42}", "{value: 42}"],
    ["42", "42"],
    ["'plain string'", "'plain string'"],
    ["Timeout", "Timeout"],
    ["undefined", "undefined"],
    ["", ""],
  ];

  for (const [input, expected] of cases) {
    it(`renders ${JSON.stringify(input)} → ${JSON.stringify(expected)}`, () => {
      assert.strictEqual(renderBynkValue(input), expected);
    });
  }

  it("never throws on malformed input", () => {
    for (const junk of ["{tag: 'X'", "{{{{", "{tag: }", "{,,,}", "{tag: '\\'}", "{…", "[}"]) {
      assert.doesNotThrow(() => renderBynkValue(junk));
    }
  });
});
