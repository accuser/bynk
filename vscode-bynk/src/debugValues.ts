// Slice 5 (debugging track, Phase 2): render Bynk's algebraic values in the
// debugger in Bynk's own vocabulary — `Ok(42)`, not `{ tag: "Ok", value: 42 }`.
//
// Bynk's `Result`/`Option`/sum values lower to uniformly *tagged objects* (see
// bynk-emit's runtime): `Ok(v)` is `{ tag: "Ok", value: v }`, `None` is
// `{ tag: "None" }`, a sum variant is `{ tag: "Name", ...fields }`. We hand
// js-debug a `customDescriptionGenerator` — a function it evaluates *in the
// debuggee* for every object (`this` = the object, first arg = the description
// it would otherwise show) — that recognises that shape and renders Bynk
// constructor syntax, returning the default for everything else.
//
// Constraints the generator must meet (it runs on *every* described object):
//   - total + side-effect-free (wrapped in try/catch, returns the default on any
//     surprise);
//   - cheap (a single `typeof this.tag === "string"` guard rejects non-ADTs);
//   - runtime-broad (ES5-ish — it runs under Node *and* workerd's V8).
// Recognition is structural (`this.tag`); a runtime brand is the escape hatch if
// that proves to false-positive (proposal DECISION D).

/** The `customDescriptionGenerator` function source js-debug evaluates in the
 *  debuggee. Kept as a module constant so the spike/integration tests exercise
 *  the exact string the provider injects. */
export const BYNK_DESCRIPTION_GENERATOR = `function (defaultValue) {
  function render(v, d) {
    if (typeof v === "string") return JSON.stringify(v);
    if (v === null || typeof v !== "object") return String(v);
    if (typeof v.tag === "string") {
      var ks = Object.keys(v).filter(function (k) { return k !== "tag"; });
      if (!ks.length) return v.tag;
      if (d <= 0) return v.tag + "(…)";
      return v.tag + "(" + ks.map(function (k) { return render(v[k], d - 1); }).join(", ") + ")";
    }
    if (Array.isArray(v)) return "[" + v.map(function (x) { return render(x, d - 1); }).join(", ") + "]";
    return "{…}";
  }
  try {
    return (this && typeof this.tag === "string") ? render(this, 4) : defaultValue;
  } catch (e) { return defaultValue; }
}`;
