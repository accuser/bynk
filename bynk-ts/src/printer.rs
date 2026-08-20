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

    /// Review of #1308, finding 2: nothing requires `Verbatim` text to be
    /// newline-terminated — the printer owns line structure (R7.3), so it's
    /// the one that guarantees two statements never share a generated line,
    /// not a `TsStmt::verbatim` caller obligation nobody enforces.
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
