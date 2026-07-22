// #852: one stable rendered element per participant, in declaration order.
//
// `main.ts` zips `participantOrder` (declaration order) against the rendered
// SVG to wire participant click-to-code. The naive `.actor` selector is wrong:
// Mermaid 11 emits SEVERAL `.actor`-classed elements per participant, grouped
// and ordered by kind — not one-per-participant in declaration order. Observed
// (Mermaid 11.16) for `participant api call / actor User / participant Clock`:
//
//   - a `participant` box → four `.actor` nodes: `rect.actor.actor-bottom`,
//     `text.actor.actor-box` (bottom label), `rect.actor.actor-top`,
//     `text.actor.actor-box` (top label);
//   - an `actor` stick-figure → `text.actor.actor-man` label nodes that appear
//     at the END of DOM order, after every box;
//   - within each group the boxes come out in REVERSE declaration order.
//
// So `querySelectorAll(".actor")[i]` bears no relation to participant `i`.
//
// The one signal that IS per-participant and declaration-ordered is horizontal
// position: Mermaid lays participants left-to-right in declaration order and
// centres each participant's label at the same `x` for its top and bottom
// copies. So: take every participant *label* node (`.actor-box` for boxes,
// `.actor-man` for stick-figures), collapse the top/bottom pair to one entry
// per distinct `x`, and sort by `x`. That yields exactly one element per
// participant, in declaration order — the order `participantOrder` is in.

/** One label element per participant, left-to-right (= declaration order).
 *  Length equals the participant count, so callers can zip it 1:1 against
 *  `participantOrder`. `x` is rounded to fold any sub-pixel drift between a
 *  participant's top and bottom label into a single entry (real participants
 *  are spaced far more than a pixel apart, so distinct participants never
 *  collide). */
export function participantEls(root: ParentNode): Element[] {
  const labels = root.querySelectorAll("text.actor-box, text.actor-man");
  const byX = new Map<number, Element>();
  for (const el of Array.from(labels)) {
    const x = Number(el.getAttribute("x"));
    if (!Number.isFinite(x)) continue;
    const key = Math.round(x);
    if (!byX.has(key)) byX.set(key, el);
  }
  return Array.from(byX.entries())
    .sort((a, b) => a[0] - b[0])
    .map(([, el]) => el);
}
