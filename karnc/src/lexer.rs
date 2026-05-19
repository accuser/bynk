//! Lexer for Karn v0.
//!
//! Token kinds correspond to the terminals defined in the grammar (spec §3
//! and §4). Whitespace is skipped; line comments are emitted as `Comment`
//! tokens so the formatter can preserve them through round-trips (v1.1 LSP
//! spec §3.5). Doc blocks (`---`) are emitted as `DocBlock` tokens, lexed
//! outside of logos (see [`tokenize`]).

use logos::Logos;

use crate::error::CompileError;
use crate::span::Span;

/// Token kinds. Discriminants without payload data; the lexeme is recovered
/// from the source string via the token's [`Span`].
///
/// Note: `--` line comments and `---` doc block markers are handled outside
/// logos (see [`tokenize`]), because doc blocks are delimited by `---` lines
/// containing only the marker and may span multiple source lines.
#[derive(Logos, Debug, Clone, Copy, PartialEq, Eq)]
#[logos(skip r"[ \t\r\n]+")]
pub enum TokenKind {
    // Keywords
    #[token("commons")]
    Commons,
    #[token("type")]
    Type,
    #[token("fn")]
    Fn,
    #[token("where")]
    Where,
    #[token("and")]
    And,
    #[token("true")]
    True,
    #[token("false")]
    False,
    #[token("Int")]
    Int,
    #[token("String")]
    String,
    #[token("Bool")]
    Bool,
    // v0.1 keywords
    #[token("let")]
    Let,
    #[token("if")]
    If,
    #[token("else")]
    Else,
    #[token("Ok")]
    Ok,
    #[token("Err")]
    Err,
    #[token("Result")]
    Result,
    #[token("ValidationError")]
    ValidationError,
    // v0.2 keywords
    #[token("enum")]
    Enum,
    #[token("match")]
    Match,
    #[token("Option")]
    Option,
    #[token("record")]
    Record,
    #[token("self")]
    Self_,
    #[token("Some")]
    Some,
    #[token("None")]
    None,
    #[token("is")]
    Is,
    // v0.3 keywords
    #[token("opaque")]
    Opaque,
    #[token("uses")]
    Uses,
    // v0.4 keywords
    #[token("context")]
    Context,
    #[token("consumes")]
    Consumes,
    #[token("exports")]
    Exports,
    #[token("transparent")]
    Transparent,
    // v0.5 keywords
    #[token("agent")]
    Agent,
    #[token("capability")]
    Capability,
    #[token("commit")]
    Commit,
    #[token("Effect")]
    Effect,
    #[token("given")]
    Given,
    #[token("on")]
    On,
    #[token("provides")]
    Provides,
    #[token("service")]
    Service,
    #[token("state")]
    State,
    /// `...` — used in record-spread expressions (v0.5).
    #[token("...")]
    DotDotDot,
    /// `<-` — Effect bind operator (v0.5).
    #[token("<-")]
    LArrow,

    /// A documentation block: `---` line ... `---` line. The token's span
    /// covers the full block including both `---` markers. The body content
    /// is recovered from the source via the span (see [`doc_block_content`]).
    /// Inserted by [`tokenize`]; not lexed by logos directly.
    DocBlock,

    /// A line comment: `-- ...` running to end of line. The span starts at
    /// the `--` marker and runs through the last character before the
    /// terminating newline (exclusive). The trivia body (the text after the
    /// `--` marker) is recovered from the source via the span. Inserted by
    /// [`tokenize`]; not lexed by logos directly so it cannot be mistaken
    /// for an `--` operator sequence.
    Comment,

    // Identifier
    #[regex(r"[A-Za-z][A-Za-z0-9_]*")]
    Ident,

    // Literals
    #[regex(r"[0-9]+")]
    IntLit,
    // A double-quoted string with simple escapes. The body excludes the closing
    // quote; we accept any non-quote/non-backslash/non-newline char, or a
    // backslash followed by one of the four allowed escapes.
    #[regex(r#""([^"\\\n]|\\[nt"\\])*""#)]
    StrLit,

    // Multi-char operators
    #[token("->")]
    Arrow,
    #[token("==")]
    EqEq,
    #[token("!=")]
    BangEq,
    #[token("<=")]
    LtEq,
    #[token(">=")]
    GtEq,
    #[token("&&")]
    AmpAmp,
    #[token("||")]
    PipePipe,

    // Single-char operators
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("!")]
    Bang,
    #[token("=")]
    Eq,
    #[token("<")]
    Lt,
    #[token(">")]
    Gt,
    // v0.1 postfix operator
    #[token("?")]
    Question,
    // v0.2 match-arm arrow
    #[token("=>")]
    FatArrow,
    // v0.2 wildcard pattern (also valid as identifier start; the lexer
    // prefers identifier for any longer match, so `_foo` is still Ident).
    #[token("_")]
    Underscore,
    // v0.2 sum-type variant separator (also used as future bitwise OR);
    // single `|` distinct from `||`.
    #[token("|")]
    Pipe,

    // Punctuation
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token(",")]
    Comma,
    #[token(":")]
    Colon,
    #[token(".")]
    Dot,
}

impl TokenKind {
    /// Human-readable display name for diagnostics.
    pub fn describe(self) -> &'static str {
        use TokenKind::*;
        match self {
            Commons => "`commons`",
            Type => "`type`",
            Fn => "`fn`",
            Where => "`where`",
            And => "`and`",
            True => "`true`",
            False => "`false`",
            Int => "`Int`",
            String => "`String`",
            Bool => "`Bool`",
            Let => "`let`",
            If => "`if`",
            Else => "`else`",
            Ok => "`Ok`",
            Err => "`Err`",
            Result => "`Result`",
            ValidationError => "`ValidationError`",
            Enum => "`enum`",
            Match => "`match`",
            Option => "`Option`",
            Record => "`record`",
            Self_ => "`self`",
            Some => "`Some`",
            None => "`None`",
            Is => "`is`",
            Opaque => "`opaque`",
            Uses => "`uses`",
            Context => "`context`",
            Consumes => "`consumes`",
            Exports => "`exports`",
            Transparent => "`transparent`",
            Agent => "`agent`",
            Capability => "`capability`",
            Commit => "`commit`",
            Effect => "`Effect`",
            Given => "`given`",
            On => "`on`",
            Provides => "`provides`",
            Service => "`service`",
            State => "`state`",
            DotDotDot => "`...`",
            LArrow => "`<-`",
            DocBlock => "documentation block",
            Comment => "line comment",
            Ident => "identifier",
            IntLit => "integer literal",
            StrLit => "string literal",
            Arrow => "`->`",
            EqEq => "`==`",
            BangEq => "`!=`",
            LtEq => "`<=`",
            GtEq => "`>=`",
            AmpAmp => "`&&`",
            PipePipe => "`||`",
            Plus => "`+`",
            Minus => "`-`",
            Star => "`*`",
            Slash => "`/`",
            Bang => "`!`",
            Eq => "`=`",
            Lt => "`<`",
            Gt => "`>`",
            Question => "`?`",
            FatArrow => "`=>`",
            Underscore => "`_`",
            Pipe => "`|`",
            LParen => "`(`",
            RParen => "`)`",
            LBrace => "`{`",
            RBrace => "`}`",
            LBracket => "`[`",
            RBracket => "`]`",
            Comma => "`,`",
            Colon => "`:`",
            Dot => "`.`",
        }
    }
}

/// A token plus its source span.
#[derive(Debug, Clone, Copy)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

/// Tokenise a source string. Returns the full token vector or the first
/// lexical error.
///
/// Doc blocks (`---` ... `---`) and line comments (`-- ...`) are recognised
/// outside the logos-generated lexer: we scan the source one segment at a
/// time, dispatching to logos for ordinary tokens between non-token spans.
pub fn tokenize(source: &str) -> Result<Vec<Token>, CompileError> {
    let mut tokens = Vec::new();
    let bytes = source.as_bytes();
    let mut pos = 0;
    while pos < bytes.len() {
        // Detect a `---` doc-block marker at the start of a line (the line may
        // begin with leading whitespace; the marker itself must be alone on
        // its line).
        if let Some(open_end) = doc_block_open_at(source, pos) {
            // Find the matching closing `---` line.
            match doc_block_close(source, open_end) {
                Some((close_start, close_end)) => {
                    let span = Span::new(pos, close_end);
                    tokens.push(Token {
                        kind: TokenKind::DocBlock,
                        span,
                    });
                    let _ = close_start;
                    pos = close_end;
                    continue;
                }
                None => {
                    return Err(CompileError::new(
                        "karn.lex.unclosed_doc_block",
                        Span::new(pos, open_end),
                        "documentation block opened but never closed",
                    )
                    .with_note(
                        "a doc block must be terminated by another `---` on a line by itself",
                    ));
                }
            }
        }
        // A `--` line comment: emit a `Comment` token covering everything
        // up to (but not including) the terminating newline. Doc-block
        // detection above already ruled out a `---` marker at line start
        // — and once we've consumed past the leading `--`, any further
        // dashes are part of the comment body. Preserving comments as
        // trivia tokens lets the parser attach them to declarations so
        // the formatter can emit them in place (v1.1 LSP spec §3.5).
        if pos + 1 < bytes.len() && bytes[pos] == b'-' && bytes[pos + 1] == b'-' {
            let start = pos;
            while pos < bytes.len() && bytes[pos] != b'\n' {
                pos += 1;
            }
            tokens.push(Token {
                kind: TokenKind::Comment,
                span: Span::new(start, pos),
            });
            continue;
        }
        // Skip ordinary whitespace inline (logos handles it too, but we may
        // be in the middle of the source between specials).
        if matches!(bytes[pos], b' ' | b'\t' | b'\r' | b'\n') {
            pos += 1;
            continue;
        }
        // Otherwise dispatch a single logos token starting at `pos`.
        let mut lex = TokenKind::lexer(&source[pos..]);
        let Some(result) = lex.next() else {
            // No token at this position; treat as unexpected character so
            // the user sees something useful.
            let ch = source[pos..].chars().next().unwrap_or('\0');
            let span = Span::new(pos, pos + ch.len_utf8());
            return Err(CompileError::new(
                "karn.lex.unexpected_character",
                span,
                format!("unexpected character `{ch}`"),
            ));
        };
        let local = lex.span();
        let span: Span = Span::new(pos + local.start, pos + local.end);
        match result {
            Ok(kind) => {
                if kind == TokenKind::IntLit {
                    let slice = &source[span.range()];
                    if slice.parse::<i64>().is_err() {
                        return Err(CompileError::new(
                            "karn.lex.integer_overflow",
                            span,
                            format!(
                                "integer literal `{slice}` is out of range for a 64-bit signed integer"
                            ),
                        )
                        .with_note("the range is -2^63 to 2^63 - 1"));
                    }
                }
                tokens.push(Token { kind, span });
                pos = span.end;
            }
            Err(()) => {
                let slice = &source[span.range()];
                let ch = slice.chars().next().unwrap_or('\0');
                let err = if ch == '"' {
                    CompileError::new(
                        "karn.lex.unterminated_string",
                        span,
                        "unterminated string literal",
                    )
                    .with_note(
                        "string literals must close with `\"` on the same line; \
                         supported escapes are `\\n`, `\\t`, `\\\"`, `\\\\`",
                    )
                } else {
                    CompileError::new(
                        "karn.lex.unexpected_character",
                        span,
                        format!("unexpected character `{ch}`"),
                    )
                };
                return Err(err);
            }
        }
    }
    Ok(tokens)
}

/// If a `---` doc-block marker line starts at or shortly after `pos` (which
/// must be at a line boundary), return the byte offset just past the marker
/// line (after the terminating newline, or at EOF). The doc-block grammar
/// requires the marker to be alone on its line; leading horizontal whitespace
/// is allowed and ignored.
fn doc_block_open_at(source: &str, pos: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    if !at_line_start(source, pos) {
        return None;
    }
    // Skip leading horizontal whitespace.
    let mut i = pos;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    if i + 3 > bytes.len() {
        return None;
    }
    if &bytes[i..i + 3] != b"---" {
        return None;
    }
    i += 3;
    // The marker may have additional trailing dashes (per spec "three or more
    // consecutive hyphens"). Consume them.
    while i < bytes.len() && bytes[i] == b'-' {
        i += 1;
    }
    // After the dashes, allow only horizontal whitespace then newline/EOF.
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t' || bytes[i] == b'\r') {
        i += 1;
    }
    if i == bytes.len() {
        return Some(i);
    }
    if bytes[i] == b'\n' {
        return Some(i + 1);
    }
    None
}

/// Find the next closing `---` line at or after `pos`. Returns
/// `(start_of_line, end_of_line)` (`end_of_line` is just past the
/// terminating newline, or at EOF).
fn doc_block_close(source: &str, mut pos: usize) -> Option<(usize, usize)> {
    let bytes = source.as_bytes();
    while pos < bytes.len() {
        // Advance pos to the start of a line.
        let line_start = pos;
        // Find the end of this line.
        let mut line_end = line_start;
        while line_end < bytes.len() && bytes[line_end] != b'\n' {
            line_end += 1;
        }
        // Check this line.
        if let Some(end) = doc_block_open_at(source, line_start) {
            return Some((line_start, end));
        }
        // Move to the next line.
        pos = if line_end < bytes.len() {
            line_end + 1
        } else {
            line_end
        };
    }
    None
}

/// Returns true if byte offset `pos` is at a line start (column 0).
fn at_line_start(source: &str, pos: usize) -> bool {
    if pos == 0 {
        return true;
    }
    let bytes = source.as_bytes();
    bytes[pos - 1] == b'\n'
}

/// Extract the body content of a doc-block token from its source span.
/// Strips the leading and trailing `---` marker lines and returns the body
/// verbatim. If every non-empty content line begins with the same horizontal
/// whitespace prefix (e.g., because the doc block sits inside a brace-form
/// commons body), that common prefix is removed so the body reads naturally
/// when emitted as JSDoc.
pub fn doc_block_content(source: &str, span: Span) -> String {
    let slice = &source[span.range()];
    // Drop the first line (opening marker).
    let after_open = match slice.find('\n') {
        Some(i) => &slice[i + 1..],
        None => return String::new(),
    };
    let bytes = after_open.as_bytes();
    // Trim the trailing closing-marker line.
    let mut i = bytes.len();
    if i > 0 && bytes[i - 1] == b'\n' {
        i -= 1;
    }
    while i > 0 && matches!(bytes[i - 1], b' ' | b'\t' | b'\r') {
        i -= 1;
    }
    while i > 0 && bytes[i - 1] == b'-' {
        i -= 1;
    }
    if i > 0 && bytes[i - 1] == b'\n' {
        i -= 1;
    }
    let body = &after_open[..i];

    // Compute the common leading-whitespace prefix across all non-empty lines
    // and strip it. This lets writers indent the doc block alongside the
    // declaration it documents without bleeding the indent into the JSDoc.
    let common: Option<usize> = body
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.bytes().take_while(|&b| b == b' ' || b == b'\t').count())
        .min();
    let strip = common.unwrap_or(0);
    if strip == 0 {
        return body.to_string();
    }
    let mut out = String::with_capacity(body.len());
    let mut first = true;
    for line in body.lines() {
        if !first {
            out.push('\n');
        }
        first = false;
        if line.trim().is_empty() {
            // Preserve blank lines.
            continue;
        }
        let leading: usize = line
            .bytes()
            .take_while(|&b| b == b' ' || b == b'\t')
            .count();
        let drop = strip.min(leading);
        out.push_str(&line[drop..]);
    }
    out
}

/// Extract the body of a `Comment` trivia token: everything after the
/// leading `--` marker, preserving its inline whitespace verbatim. Used by
/// the parser when attaching comments to declarations.
pub fn comment_body(source: &str, span: Span) -> &str {
    let slice = &source[span.range()];
    // Strip leading "--" if present (defensive — the lexer always emits
    // Comment tokens whose span begins with `--`).
    slice.strip_prefix("--").unwrap_or(slice)
}

/// Returns true if there is a blank line (a line containing only whitespace)
/// in `source` strictly between byte offsets `from` (inclusive) and `to`
/// (exclusive). Used by the parser to detect orphan doc blocks.
///
/// A doc-block token's span ends just past the closing-marker line's
/// terminating newline. So if the next declaration begins on the immediately
/// following line, the substring between contains no newline (only optional
/// indentation). Any newline in the substring therefore implies at least one
/// entirely-blank line separating the doc from the declaration.
pub fn has_blank_line_between(source: &str, from: usize, to: usize) -> bool {
    if to <= from {
        return false;
    }
    let bytes = source.as_bytes();
    let mut i = from;
    while i < to {
        if bytes[i] == b'\n' {
            return true;
        }
        if !matches!(bytes[i], b' ' | b'\t' | b'\r') {
            return false;
        }
        i += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(source: &str) -> Vec<TokenKind> {
        tokenize(source)
            .unwrap()
            .into_iter()
            .map(|t| t.kind)
            .collect()
    }

    #[test]
    fn keywords_and_idents() {
        use TokenKind::*;
        assert_eq!(
            kinds("commons type fn where and true false Int String Bool foo bar"),
            vec![
                Commons, Type, Fn, Where, And, True, False, Int, String, Bool, Ident, Ident
            ],
        );
    }

    #[test]
    fn integer_and_string_literals() {
        use TokenKind::*;
        assert_eq!(
            kinds(r#"0 42 "hello" "with\nescape""#),
            vec![IntLit, IntLit, StrLit, StrLit]
        );
    }

    #[test]
    fn operators() {
        use TokenKind::*;
        assert_eq!(
            kinds("-> == != <= >= && || + - * / ! = < > ( ) { } [ ] , : ."),
            vec![
                Arrow, EqEq, BangEq, LtEq, GtEq, AmpAmp, PipePipe, Plus, Minus, Star, Slash, Bang,
                Eq, Lt, Gt, LParen, RParen, LBrace, RBrace, LBracket, RBracket, Comma, Colon, Dot,
            ],
        );
    }

    #[test]
    fn line_comments_emitted_as_trivia() {
        // v1.1: line comments are preserved as Comment tokens so the
        // formatter can attach and re-emit them.
        use TokenKind::*;
        let src = "-- a comment\ntype X = Int -- trailing\n";
        assert_eq!(kinds(src), vec![Comment, Type, Ident, Eq, Int, Comment],);
    }

    #[test]
    fn comment_body_extracts_text_after_marker() {
        let toks = tokenize("-- hello world\n").unwrap();
        assert_eq!(toks.len(), 1);
        assert_eq!(toks[0].kind, TokenKind::Comment);
        assert_eq!(
            comment_body("-- hello world\n", toks[0].span),
            " hello world"
        );
    }

    #[test]
    fn comment_does_not_consume_newline() {
        // Two adjacent comment lines should produce two distinct tokens
        // — the newline between them is not part of either comment's span.
        let toks = tokenize("-- one\n-- two\n").unwrap();
        assert_eq!(toks.len(), 2);
        assert!(toks.iter().all(|t| t.kind == TokenKind::Comment));
    }

    #[test]
    fn unterminated_string_is_error() {
        let err = tokenize("\"oops\n").unwrap_err();
        assert_eq!(err.category, "karn.lex.unterminated_string");
    }

    #[test]
    fn integer_overflow_is_error() {
        let err = tokenize("99999999999999999999").unwrap_err();
        assert_eq!(err.category, "karn.lex.integer_overflow");
    }

    #[test]
    fn unexpected_character_is_error() {
        let err = tokenize("type X = Int $").unwrap_err();
        assert_eq!(err.category, "karn.lex.unexpected_character");
    }

    #[test]
    fn v0_1_keywords() {
        use TokenKind::*;
        assert_eq!(
            kinds("let if else Ok Err Result ValidationError"),
            vec![Let, If, Else, Ok, Err, Result, ValidationError],
        );
    }

    #[test]
    fn question_token() {
        use TokenKind::*;
        assert_eq!(kinds("x?"), vec![Ident, Question]);
    }

    #[test]
    fn v0_2_keywords() {
        use TokenKind::*;
        assert_eq!(
            kinds("enum match Option record self Some None is"),
            vec![Enum, Match, Option, Record, Self_, Some, None, Is],
        );
    }

    #[test]
    fn pipe_and_pipe_pipe_disambiguated() {
        use TokenKind::*;
        assert_eq!(kinds("| || |"), vec![Pipe, PipePipe, Pipe]);
    }

    #[test]
    fn fat_arrow_and_underscore() {
        use TokenKind::*;
        assert_eq!(kinds("_ =>"), vec![Underscore, FatArrow]);
    }
}
