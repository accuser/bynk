//! Byte-offset ↔ LSP position conversion.
//!
//! Bynk source spans are byte offsets into the UTF-8 source. LSP positions
//! use UTF-16 code units (per the protocol's default position encoding).
//! For ASCII-only Bynk sources the two agree, but we go through code points
//! to handle multi-byte characters correctly in identifiers and strings.

use bynk_syntax::span::Span;
use tower_lsp::lsp_types::{Position, Range};

/// Convert a byte offset into the source string into an LSP position.
pub fn offset_to_position(source: &str, offset: usize) -> Position {
    let mut line: u32 = 0;
    let mut column: u32 = 0;
    let bytes = source.as_bytes();
    let limit = offset.min(bytes.len());
    let mut i = 0;
    while i < limit {
        let b = bytes[i];
        if b == b'\n' {
            line += 1;
            column = 0;
            i += 1;
            continue;
        }
        // Move to next UTF-8 code point boundary.
        let cp_len = utf8_char_len(b);
        // LSP default encoding is UTF-16; count UTF-16 code units.
        // For ASCII (1 byte) and 2/3-byte UTF-8 (1 code unit) we increment
        // column by 1; for 4-byte UTF-8 (supplementary plane) it's 2 code
        // units.
        column += if cp_len == 4 { 2 } else { 1 };
        i += cp_len;
    }
    Position {
        line,
        character: column,
    }
}

/// Convert an LSP position into a byte offset. Returns None if the position
/// is past the end of the source.
pub fn position_to_offset(source: &str, position: Position) -> Option<usize> {
    let target_line = position.line;
    let target_char = position.character;
    let mut line: u32 = 0;
    let mut character: u32 = 0;
    let bytes = source.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if line == target_line && character == target_char {
            return Some(i);
        }
        let b = bytes[i];
        if b == b'\n' {
            if line == target_line {
                // Position is past end of this line; clamp to line end.
                return Some(i);
            }
            line += 1;
            character = 0;
            i += 1;
            continue;
        }
        let cp_len = utf8_char_len(b);
        character += if cp_len == 4 { 2 } else { 1 };
        i += cp_len;
    }
    if line == target_line && character >= target_char {
        Some(i)
    } else {
        None
    }
}

fn utf8_char_len(first: u8) -> usize {
    if first < 0x80 {
        1
    } else if first < 0xC0 {
        // Continuation byte; should not be the first byte of a char.
        1
    } else if first < 0xE0 {
        2
    } else if first < 0xF0 {
        3
    } else {
        4
    }
}

/// Convert a compiler [`Span`] into an LSP [`Range`].
pub fn span_to_range(source: &str, span: Span) -> Range {
    Range {
        start: offset_to_position(source, span.start),
        end: offset_to_position(source, span.end),
    }
}

/// The position one past the end of the source — used for "replace whole
/// document" formatting edits.
pub fn end_position(source: &str) -> Position {
    offset_to_position(source, source.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_offsets_match_columns() {
        let src = "abc\ndef";
        assert_eq!(offset_to_position(src, 0), Position::new(0, 0));
        assert_eq!(offset_to_position(src, 2), Position::new(0, 2));
        assert_eq!(offset_to_position(src, 4), Position::new(1, 0));
        assert_eq!(offset_to_position(src, 6), Position::new(1, 2));
    }

    #[test]
    fn position_round_trip() {
        let src = "alpha\n  beta\ngamma";
        let p = Position::new(1, 4);
        let off = position_to_offset(src, p).unwrap();
        assert_eq!(offset_to_position(src, off), p);
    }
}
