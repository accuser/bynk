//! The printer — `TsProgram -> Artefacts` (R7.3: "printing is `TsProgram ->
//! Artefacts`. The printer owns the buffer, the indentation and the offset
//! arithmetic. It is the only code in the compiler that writes a
//! character."). Owns one buffer for the whole [`TsProgram`], so unlike
//! `bynk-emit`'s own splice-based lowering (`crate::source_map`'s own module
//! doc), it never needs to rebase a sub-buffer's checkpoints — it records
//! directly from each statement's own `span` as it writes, which is what
//! makes R7.4 ("the source map is produced by the printer from
//! `TsNode.span`. No phase before the printer records an offset") true by
//! construction for this path, from this slice.
//!
//! # Readability policy (R7.5)
//!
//! R7.5: "Readable output is a printer policy with a name and a test, not a
//! property of how carefully strings were typed." This is that policy,
//! named — today's whole surface of it, not a forward guess at what P7.8's
//! real `TsStmt`/`TsExpr`/`TsType`/`TsDecl` nodes will need:
//!
//! - **Every statement starts on its own generated line, so two statements
//!   can never share one.** After writing each statement's own text,
//!   [`print()`] appends a trailing `\n` unless the *buffer* — not that
//!   statement's own text — already ends in one; this is what stops the
//!   `SourceMapBuilder`'s one-checkpoint-per-line forward pass from
//!   silently dropping an earlier statement's checkpoint (below). It is
//!   *not* "one generated line per statement": `VerbatimOrigin::
//!   NotYetConverted` wraps a whole multi-line document per statement (the
//!   ordinary shape today — every P7.6 construction site does this), so one
//!   statement routinely spans dozens of lines; and an empty statement
//!   contributes no line of its own at all when the buffer already ends in
//!   `\n` (`prints_every_statement_in_order` /
//!   `a_statement_missing_its_own_trailing_newline_still_gets_its_own_line`
//!   / `a_multi_line_statement_still_starts_the_next_statement_on_a_fresh_line`
//!   pin the ordinary cases; `an_empty_statement_first_in_the_program_yields_
//!   a_leading_blank_line` /
//!   `an_empty_statement_after_a_newline_terminated_one_contributes_no_line_
//!   of_its_own` pin the two edge behaviours the buffer-vs-statement-text
//!   distinction actually produces).
//! - **A statement's own interior is not the printer's concern — yet.**
//!   Every `TsStmt` today is a `VerbatimOrigin`-tagged opaque string; its
//!   indentation, spacing, and brace placement are whatever the
//!   still-untreed `bynk-emit` call site that built it produced, not a
//!   choice this printer makes. Indentation, blank-line placement between
//!   declarations, and brace style become real printer decisions — and
//!   this policy grows to name them — only once P7.8 gives the printer
//!   actual structured nodes to have an opinion about; inventing rules for
//!   shapes that don't exist yet would be guessing, not designing, the same
//!   trap this crate's own module doc (`lib.rs`) already named for the
//!   tree's *structure* — applied here to its *formatting*.

use crate::program::TsProgram;
use crate::source_map::SourceMapBuilder;

/// The result of printing a [`TsProgram`]: the emitted text, and its source
/// map — `None` when no checkpoint resolved, either because no statement
/// carried a span or every span fell outside `source_text`
/// ([`SourceMapBuilder::to_v3`]'s own "nothing resolves" case).
#[derive(Debug)]
pub struct Printed {
    pub text: String,
    pub source_map: Option<String>,
}

/// Print `program` to TypeScript text. `source_name`/`source_text` register
/// the `.bynk` source every statement's own span is measured against —
/// today always exactly one, since nothing spans two files yet;
/// `output_file` names the generated file in the source map's own `file`
/// field.
pub fn print(
    program: &TsProgram,
    source_name: &str,
    source_text: &str,
    output_file: &str,
) -> Printed {
    let mut out = String::new();
    let mut map = SourceMapBuilder::new();
    map.add_source(source_name, source_text);
    for stmt in &program.stmts {
        if let Some(span) = stmt.span {
            map.record(out.len(), span);
        }
        out.push_str(stmt.text());
        // The printer owns line structure (R7.3), so a statement's own text
        // not ending in its own newline can't leave two statements sharing
        // a generated line — review of #1308, finding 2: nothing required
        // `Verbatim` text to be newline-terminated, and two that weren't
        // would jam onto one line *and* silently lose the earlier
        // statement's own checkpoint (`SourceMapBuilder::record`'s
        // same-offset dedup, and `to_v3`'s one-checkpoint-per-line forward
        // pass, both keep only the later one).
        if !out.ends_with('\n') {
            out.push('\n');
        }
    }
    let source_map = map.to_v3(&out, output_file);
    Printed {
        text: out,
        source_map,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::program::{TsStmt, VerbatimOrigin};
    use bynk_syntax::span::Span;

    /// Pins the readability policy's statement-separation guarantee (R7.5,
    /// this module's own doc): every statement starts on its own generated
    /// line.
    #[test]
    fn prints_every_statement_in_order() {
        let mut program = TsProgram::new();
        program.push(TsStmt::verbatim(
            VerbatimOrigin::Contracts,
            "const a = 1;\n",
            None,
        ));
        program.push(TsStmt::verbatim(
            VerbatimOrigin::Secrets,
            "const b = 2;\n",
            None,
        ));
        let printed = print(&program, "x.bynk", "", "x.ts");
        assert_eq!(printed.text, "const a = 1;\nconst b = 2;\n");
    }

    /// Pins the readability policy's statement-separation guarantee (R7.5,
    /// this module's own doc) in its sharpest form: nothing requires
    /// `Verbatim` text to be newline-terminated — the printer owns line
    /// structure (R7.3), so it's the one that guarantees two statements
    /// never share a generated line, not a `TsStmt::verbatim` caller
    /// obligation nobody enforces (review of #1308, finding 2).
    #[test]
    fn a_statement_missing_its_own_trailing_newline_still_gets_its_own_line() {
        let mut program = TsProgram::new();
        program.push(TsStmt::verbatim(
            VerbatimOrigin::Contracts,
            "const a = 1;",
            None,
        ));
        program.push(TsStmt::verbatim(
            VerbatimOrigin::Secrets,
            "const b = 2;",
            None,
        ));
        let printed = print(&program, "x.bynk", "", "x.ts");
        assert_eq!(printed.text, "const a = 1;\nconst b = 2;\n");
    }

    /// Production `Verbatim` content is rarely one line — `VerbatimOrigin::
    /// NotYetConverted` (P7.6) wraps a whole generated document per
    /// statement, so one statement routinely spans dozens of generated
    /// lines. Neither of the two tests above exercised that shape (review
    /// of #1312, finding 1) — this one confirms the readability policy's
    /// real invariant (the *next* statement always starts on a fresh line)
    /// holds for multi-line content too, not just the single-line text
    /// those tests happened to use.
    #[test]
    fn a_multi_line_statement_still_starts_the_next_statement_on_a_fresh_line() {
        let mut program = TsProgram::new();
        program.push(TsStmt::verbatim(
            VerbatimOrigin::Contracts,
            "function a() {\n  return 1;\n}\n",
            None,
        ));
        program.push(TsStmt::verbatim(
            VerbatimOrigin::Secrets,
            "const b = 2;\n",
            None,
        ));
        let printed = print(&program, "x.bynk", "", "x.ts");
        assert_eq!(
            printed.text,
            "function a() {\n  return 1;\n}\nconst b = 2;\n"
        );
    }

    /// `print` decides whether to append a trailing newline by checking the
    /// whole *buffer*, not the statement's own text (review of #1312,
    /// finding 2) — an empty statement's text changes nothing, so whether
    /// it gets a line depends entirely on what's already in the buffer.
    /// First in a program, the buffer is still empty, so the check fires
    /// and the output opens with a blank line. Pinned here so this edge
    /// stays a known, deliberate reading of the code rather than an
    /// untested accident.
    #[test]
    fn an_empty_statement_first_in_the_program_yields_a_leading_blank_line() {
        let mut program = TsProgram::new();
        program.push(TsStmt::verbatim(VerbatimOrigin::Contracts, "", None));
        program.push(TsStmt::verbatim(
            VerbatimOrigin::Secrets,
            "const b = 2;\n",
            None,
        ));
        let printed = print(&program, "x.bynk", "", "x.ts");
        assert_eq!(printed.text, "\nconst b = 2;\n");
    }

    /// The other half of the buffer-vs-statement-text distinction (review
    /// of #1312, finding 2): an empty statement following one that already
    /// ended its own text in `\n` contributes no line at all — the buffer
    /// already ends in `\n`, so the check doesn't fire, and the empty
    /// statement's own (non-)content and the next statement's text end up
    /// on what reads as one generated line for the next statement alone.
    #[test]
    fn an_empty_statement_after_a_newline_terminated_one_contributes_no_line_of_its_own() {
        let mut program = TsProgram::new();
        program.push(TsStmt::verbatim(
            VerbatimOrigin::Contracts,
            "const a = 1;\n",
            None,
        ));
        program.push(TsStmt::verbatim(VerbatimOrigin::Secrets, "", None));
        program.push(TsStmt::verbatim(
            VerbatimOrigin::RuntimeUse,
            "const c = 3;\n",
            None,
        ));
        let printed = print(&program, "x.bynk", "", "x.ts");
        assert_eq!(printed.text, "const a = 1;\nconst c = 3;\n");
    }

    #[test]
    fn no_spans_at_all_means_no_source_map() {
        let mut program = TsProgram::new();
        program.push(TsStmt::verbatim(VerbatimOrigin::Contracts, "x;\n", None));
        let printed = print(&program, "x.bynk", "let x = 1\n", "x.ts");
        assert_eq!(printed.source_map, None);
    }

    /// The property that actually matters (R7.4): the printer's own
    /// buffer-position bookkeeping is correct — a checkpoint recorded from a
    /// later statement's span still resolves to the *generated* line that
    /// statement's own text landed on, not (as the old splice-based
    /// mechanism could get wrong, #4) some other buffer's offset.
    ///
    /// Review of #1308, finding 3: the original version of this test
    /// asserted only that the source map's `sources` array was present —
    /// true even with the second statement's checkpoint silently dropped,
    /// exactly the failure this doc comment claims is ruled out. Asserting
    /// full equality against a map built independently (replicating the
    /// printer's own record-then-write order by hand, not calling `print`)
    /// actually pins both statements' own offsets.
    #[test]
    fn each_statements_span_resolves_to_its_own_generated_line() {
        let source = "let a = 1\nlet b = 2\n";
        let off_a = source.find("let a").unwrap();
        let off_b = source.find("let b").unwrap();
        let span_a = Span::new(off_a, off_a + 5);
        let span_b = Span::new(off_b, off_b + 5);
        let text_a = "const a = 1;\n";
        let text_b = "const b = 2;\n";
        let mut program = TsProgram::new();
        program.push(TsStmt::verbatim(
            VerbatimOrigin::Contracts,
            text_a,
            Some(span_a),
        ));
        program.push(TsStmt::verbatim(
            VerbatimOrigin::Secrets,
            text_b,
            Some(span_b),
        ));

        let printed = print(&program, "x.bynk", source, "x.ts");

        let mut expected_map = SourceMapBuilder::new();
        expected_map.add_source("x.bynk", source);
        expected_map.record(0, span_a);
        expected_map.record(text_a.len(), span_b);
        let expected = expected_map.to_v3(&printed.text, "x.ts");

        assert_eq!(printed.source_map, expected);
    }
}
