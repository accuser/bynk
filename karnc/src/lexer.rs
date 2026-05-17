//! Lexer for Karn v0.
//!
//! Token kinds correspond to the terminals defined in the grammar (spec §3
//! and §4). Whitespace and line comments are skipped; everything else is
//! emitted with its source span.

use logos::Logos;

use crate::error::CompileError;
use crate::span::Span;

/// Token kinds. Discriminants without payload data; the lexeme is recovered
/// from the source string via the token's [`Span`].
#[derive(Logos, Debug, Clone, Copy, PartialEq, Eq)]
#[logos(skip r"[ \t\r\n]+")]
#[logos(skip r"--[^\n]*")]
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
pub fn tokenize(source: &str) -> Result<Vec<Token>, CompileError> {
    let mut tokens = Vec::new();
    let mut lex = TokenKind::lexer(source);
    while let Some(result) = lex.next() {
        let span: Span = lex.span().into();
        match result {
            Ok(kind) => {
                if kind == TokenKind::IntLit {
                    // Range-check integer literals at lex time so we emit a precise error.
                    let slice = &source[span.range()];
                    if slice.parse::<i64>().is_err() {
                        return Err(CompileError::new(
                            "karn.lex.integer_overflow",
                            span,
                            format!("integer literal `{slice}` is out of range for a 64-bit signed integer"),
                        )
                        .with_note("the range is -2^63 to 2^63 - 1"));
                    }
                }
                tokens.push(Token { kind, span });
            }
            Err(()) => {
                let slice = &source[span.range()];
                let ch = slice.chars().next().unwrap_or('\0');
                // Distinguish a few specific lexical errors for nicer messages.
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
    fn line_comments_skipped() {
        use TokenKind::*;
        let src = "-- a comment\ntype X = Int -- trailing\n";
        assert_eq!(kinds(src), vec![Type, Ident, Eq, Int]);
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
}
