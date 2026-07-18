# 0232 — The formatter verifies its output round-trips before atomically writing it

- **Status:** Accepted (v0.208.1)

**Context.** `bynk fmt` / `bynkc fmt` (one body, `bynk_driver::run_fmt`)
overwrote each input with whatever `bynk_fmt::format_source` returned. Three
gaps made that unsafe (found in a full-repo review, #735):

1. **No round-trip guard for comment-free files.** The formatter's only
   self-check was the #523 comment-multiset guard, which returns early when the
   input holds no comments (`fmt.rs`). The printer is hand-written and dodges
   several parse traps by convention — a tail `()` re-attaching as a call, a
   trailing comma making a param list unparseable — so any AST shape the corpus
   misses that the printer mis-renders was written straight over the user's file
   with exit 0. The property tests assert the output *re-parses*, not that it
   re-parses to the *same* AST.

2. **Non-atomic write.** `std::fs::write(input, formatted)` truncates the
   destination before writing. An ENOSPC, a signal, or a crash mid-write leaves
   the file truncated or empty — and `fmt`'s only copy of the original is the
   in-memory `source`, so that original is then gone.

3. **`fmt --check -` (stdin) ignored `--check`.** The stdin path always printed
   the formatted text and never set `had_diff`, so a CI gate
   `generator | bynk fmt --check -` passed green on non-canonical input (and
   polluted the log with the reformat), while the same content in a file failed.

**Decision.**

- **Round-trip guard (`bynk_fmt::format_source`, unconditional).** After
  rendering — and after the existing comment guard — reduce both the input and
  the formatted output to their *comment-free canonical form* (`--` tokens
  dropped before parsing, so the result carries no trivia and reflects only code
  structure) and compare. The formatter only re-flows whitespace and trivia, so
  for a faithful render the two canonical strings are byte-identical. A mismatch
  means either the output no longer parses (the data-loss vector) or the printer
  altered the AST; either way `format_source` returns a `bynk.fmt.roundtrip`
  error and the caller (CLI *and* LSP — both write formatted text over the
  user's copy) leaves the file unchanged. Comparing canonical *strings* rather
  than ASTs directly avoids a span/trivia-insensitive `PartialEq` across the
  whole AST, and reuses the formatter as the canonicaliser: because the parser
  strips comments the same way, pre-filtering them changes nothing structural.
  If computing the *input's* canonical form unexpectedly fails, the guard stands
  down rather than block an otherwise-valid format on its own failure.

- **Atomic write (`bynk_driver::atomic_write`).** The in-place rewrite writes to
  a sibling temp file (same directory, so the rename stays on one filesystem;
  name carries the PID and a per-process counter so concurrent runs never
  collide), `sync_all`s it, then `rename`s it over the destination — atomic on
  POSIX and Windows, so a reader sees either the whole old file or the whole new
  one. On any failure the temp file is removed, leaving the original untouched.

- **`--check` on stdin.** The stdin path now branches on `check`: with `--check`
  it reports `<stdin> is not canonically formatted` on stderr and sets the
  diff flag (exit non-zero), without echoing the reformat; without it, it prints
  the formatted text as before.

**Consequences.** The formatter can now *refuse* on its own output — a
formatter bug surfaces as a `bynk.fmt.roundtrip` diagnostic and an unchanged
file rather than silent corruption. The guard costs an extra parse + render of
both input and output on every format (the LSP included); this is deliberate —
correctness of the user's source outweighs format latency, and the issue asked
for it unconditionally. `bynk.fmt.roundtrip`, like `bynk.fmt.comment_loss`,
lives in `bynk-fmt` and is not in the `bynkc::diagnostics` registry (the
registry scan does not cover `bynk-fmt/src`).
