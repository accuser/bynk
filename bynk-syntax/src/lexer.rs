//! Lexer for Bynk v0.
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
    // v0.21 keyword
    #[token("Float")]
    Float,
    // v0.86 keyword (ADR 0112): the `Duration` base type.
    #[token("Duration")]
    Duration,
    // v0.90 keyword (ADR 0114): the `Instant` base type.
    #[token("Instant")]
    Instant,
    // v0.110 keyword (ADR 0142): the `Bytes` base type.
    #[token("Bytes")]
    Bytes,
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
    // v0.22b keyword
    #[token("JsonError")]
    JsonError,
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
    // v0.6 keywords
    #[token("as")]
    As,
    // v0.7 keywords (v0.112: `assert`→`expect`, `test`→`suite`/`case`)
    #[token("expect")]
    Expect,
    #[token("mocks")]
    Mocks,
    #[token("suite")]
    Suite,
    #[token("case")]
    Case,
    // v0.114 keyword — generative tests (testing track slice 2). `for` and `all`
    // are deliberately *not* keywords: `all` is a list combinator (`all(xs, p)`)
    // and must stay a usable identifier. The `for all` binder is parsed
    // contextually (two identifiers) inside a `property` body instead.
    #[token("property")]
    Property,
    // v0.16 keyword
    #[token("wires")]
    Wires,
    // v0.17 keywords
    #[token("adapter")]
    Adapter,
    #[token("binding")]
    Binding,
    // v0.5 keywords
    #[token("agent")]
    Agent,
    #[token("capability")]
    Capability,
    #[token("Effect")]
    Effect,
    #[token("given")]
    Given,
    #[token("on")]
    On,
    // v0.9 keyword
    #[token("http")]
    Http,
    // v0.10a keyword
    #[token("cron")]
    Cron,
    // v0.10b keyword
    #[token("queue")]
    Queue,
    // v0.44 keywords: `from` heads a service's protocol clause; `protocol` is
    // reserved (protocols are a closed, compiler-known set — no declaration kind).
    #[token("from")]
    From,
    #[token("protocol")]
    Protocol,
    #[token("provides")]
    Provides,
    #[token("service")]
    Service,
    // v0.45 keywords: `actor` heads a boundary-contract declaration; `by`
    // heads a handler's actor clause.
    #[token("actor")]
    Actor,
    #[token("by")]
    By,
    // v0.80 keywords: `invariant` heads an agent invariant declaration; `implies`
    // is the directional logical-implication operator (`P implies Q` ≡ `!P || Q`).
    #[token("invariant")]
    Invariant,
    #[token("implies")]
    Implies,
    // v0.115 keywords — function contracts (testing track slice 3). `requires`
    // and `ensures` head a contract clause on a `fn` signature (between the
    // return type and the body). `result` is deliberately *not* a keyword: it is
    // the ordinary value name outside a contract, so it stays a usable
    // identifier; inside an `ensures` predicate it is bound contextually as the
    // function's return value (parsed by scope, like `for`/`all` in slice 2).
    // Distinct from ADR 0127's capability `@requires` annotation.
    #[token("requires")]
    Requires,
    #[token("ensures")]
    Ensures,
    // v0.116 keyword — step invariants (testing track slice 4). `transition` heads
    // an agent step-invariant declaration (beside `invariant`), a predicate over
    // the pre- and post-commit state pair. `old` and `new` are deliberately *not*
    // keywords: they stay ordinary value names outside a `transition`, and inside a
    // `transition` predicate they are bound contextually to the old/new state
    // records (parsed by scope, like `result` in an `ensures`).
    #[token("transition")]
    Transition,
    /// `...` — used in record-spread expressions (v0.5).
    #[token("...")]
    DotDotDot,
    /// `<-` — Effect bind operator (v0.5).
    #[token("<-")]
    LArrow,
    /// `~>` — asynchronous fire-and-forget send marker (v0.79). A leading
    /// statement marker, never on the RHS of a `let`; distinct from `<-` so the
    /// call site shows whether the caller waits.
    #[token("~>")]
    TildeArrow,
    /// `:=` — Cell write (v0.81, storage track). A handler statement
    /// `cell := expr`; distinct from `=` (binding) and `:` (annotation). Longer
    /// than `:`/`=` so logos matches it as one token.
    #[token(":=")]
    ColonEq,

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
    // A float literal: fraction with a digit on both sides of the `.`, an
    // exponent, or both (v0.21 §3). `1.` and `.5` are NOT float literals —
    // the digit-both-sides rule keeps `2.5.round()` / `1.toFloat()` lexing
    // as method calls on numeric literals.
    #[regex(r"[0-9]+\.[0-9]+([eE][+-]?[0-9]+)?|[0-9]+[eE][+-]?[0-9]+")]
    FloatLit,
    // A double-quoted string with simple escapes. The body excludes the closing
    // quote; we accept any non-quote/non-backslash/non-newline char, or a
    // backslash followed by one of the four allowed escapes.
    #[regex(r#""([^"\\\n]|\\[nt"\\])*""#)]
    StrLit,
    // An interpolated string `"… \(expr) …"` (v0.43). Hand-scanned in
    // `tokenize` (logos cannot balance the holes' parens), never produced by
    // the logos lexer — like [`TokenKind::DocBlock`]/[`TokenKind::Comment`].
    // The span covers the whole `"…"`; the parser splits chunks from holes.
    InterpStr,

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
    /// `@` — storage-annotation marker (v0.85, storage track; ADR 0111). Leads a
    /// `@name(args)` annotation on a `store` field (`@ttl(…)`/`@indexed(…)`); it
    /// appears only in store-field-declaration position, never as an expression
    /// operator.
    #[token("@")]
    At,

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
            Float => "`Float`",
            Duration => "`Duration`",
            Instant => "`Instant`",
            Bytes => "`Bytes`",
            Let => "`let`",
            If => "`if`",
            Else => "`else`",
            Ok => "`Ok`",
            Err => "`Err`",
            Result => "`Result`",
            ValidationError => "`ValidationError`",
            JsonError => "`JsonError`",
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
            As => "`as`",
            Expect => "`expect`",
            Mocks => "`mocks`",
            Suite => "`suite`",
            Case => "`case`",
            Property => "`property`",
            Wires => "`wires`",
            Adapter => "`adapter`",
            Binding => "`binding`",
            Agent => "`agent`",
            Capability => "`capability`",
            Effect => "`Effect`",
            Given => "`given`",
            On => "`on`",
            Http => "`http`",
            Cron => "`cron`",
            Queue => "`queue`",
            From => "`from`",
            Protocol => "`protocol`",
            Provides => "`provides`",
            Service => "`service`",
            Actor => "`actor`",
            By => "`by`",
            Invariant => "`invariant`",
            Implies => "`implies`",
            Requires => "`requires`",
            Ensures => "`ensures`",
            Transition => "`transition`",
            ColonEq => "`:=`",
            DotDotDot => "`...`",
            LArrow => "`<-`",
            TildeArrow => "`~>`",
            DocBlock => "documentation block",
            Comment => "line comment",
            Ident => "identifier",
            IntLit => "integer literal",
            FloatLit => "float literal",
            StrLit => "string literal",
            InterpStr => "interpolated string",
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
            At => "`@`",
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
                        "bynk.lex.unclosed_doc_block",
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
        // An interpolated string `"… \(expr) …"` (v0.43): only strings that
        // actually contain a `\(` hole are hand-scanned here; plain strings
        // fall through to the logos `StrLit` path unchanged. `\(` is an
        // invalid escape in the logos grammar, so this never re-routes a
        // currently-valid literal.
        if bytes[pos] == b'"' && has_interp_hole(bytes, pos) {
            let end = scan_str(bytes, source, pos)?;
            tokens.push(Token {
                kind: TokenKind::InterpStr,
                span: Span::new(pos, end),
            });
            pos = end;
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
                "bynk.lex.unexpected_character",
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
                            "bynk.lex.integer_overflow",
                            span,
                            format!(
                                "integer literal `{slice}` is out of range for a 64-bit signed integer"
                            ),
                        )
                        .with_note("the range is -2^63 to 2^63 - 1"));
                    }
                }
                if kind == TokenKind::FloatLit {
                    let slice = &source[span.range()];
                    match slice.parse::<f64>() {
                        Ok(v) if v.is_finite() => {}
                        _ => {
                            return Err(CompileError::new(
                                "bynk.lex.float_literal_overflow",
                                span,
                                format!(
                                    "float literal `{slice}` is out of range for a 64-bit float"
                                ),
                            )
                            .with_note(
                                "the literal does not fit a finite IEEE 754 double; \
                                 the largest finite value is ~1.8e308",
                            ));
                        }
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
                        "bynk.lex.unterminated_string",
                        span,
                        "unterminated string literal",
                    )
                    .with_note(
                        "string literals must close with `\"` on the same line; \
                         supported escapes are `\\n`, `\\t`, `\\\"`, `\\\\`",
                    )
                } else {
                    CompileError::new(
                        "bynk.lex.unexpected_character",
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

/// Cheap routing pre-scan (v0.43): does the string opening at `start` contain a
/// `\(` interpolation hole before it closes (or the line ends)? Decides whether
/// `tokenize` hand-scans the string as an `InterpStr` or defers to logos for a
/// plain `StrLit`. Deliberately tolerant — a malformed string with a hole is
/// routed here so the hole-aware scanner produces the precise error.
fn has_interp_hole(bytes: &[u8], start: usize) -> bool {
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' | b'"' => return false,
            b'\\' => {
                if bytes.get(i + 1) == Some(&b'(') {
                    return true;
                }
                i += 2;
            }
            _ => i += 1,
        }
    }
    false
}

/// Scan a double-quoted string starting at `start` (the opening `"`), returning
/// the byte offset just past the closing `"`. Recognises the four simple
/// escapes plus `\(…)` interpolation holes, whose parens are balanced (and
/// whose nested strings are skipped) by [`scan_hole`]. (v0.43.)
fn scan_str(bytes: &[u8], source: &str, start: usize) -> Result<usize, CompileError> {
    debug_assert_eq!(bytes[start], b'"');
    let mut i = start + 1;
    loop {
        if i >= bytes.len() || bytes[i] == b'\n' {
            return Err(CompileError::new(
                "bynk.lex.unterminated_string",
                Span::new(start, i.min(bytes.len())),
                "unterminated string literal",
            )
            .with_note(
                "string literals must close with `\"` on the same line; \
                 supported escapes are `\\n`, `\\t`, `\\\"`, `\\\\`, and `\\(…)` interpolation",
            ));
        }
        match bytes[i] {
            b'"' => return Ok(i + 1),
            b'\\' => match bytes.get(i + 1) {
                Some(b'n' | b't' | b'"' | b'\\') => i += 2,
                Some(b'(') => i = scan_hole(bytes, source, i + 2)?,
                other => {
                    let shown = other.map(|b| (*b as char).to_string()).unwrap_or_default();
                    return Err(CompileError::new(
                        "bynk.lex.bad_escape",
                        Span::new(i, (i + 2).min(bytes.len())),
                        format!("invalid escape sequence `\\{shown}` in string literal"),
                    )
                    .with_note("supported escapes: \\n \\t \\\" \\\\ \\(…)"));
                }
            },
            // Any other byte advances one position. UTF-8 continuation bytes
            // are all >= 0x80, so they never collide with the ASCII specials.
            _ => i += 1,
        }
    }
}

/// Scan an interpolation hole body. `start` points just past the `\(`; returns
/// the offset just past the matching `)`. Tracks paren depth and skips nested
/// strings (whose own parens must not close the hole), recursing through
/// [`scan_str`] so nested interpolation nests correctly. (v0.43.)
fn scan_hole(bytes: &[u8], source: &str, start: usize) -> Result<usize, CompileError> {
    let mut i = start;
    let mut depth = 1usize;
    loop {
        if i >= bytes.len() || bytes[i] == b'\n' {
            return Err(CompileError::new(
                "bynk.lex.unterminated_interpolation",
                Span::new(start.saturating_sub(2), i.min(bytes.len())),
                "unterminated interpolation hole",
            )
            .with_note(
                "an interpolation hole `\\(…)` must close with a matching `)` on the same line",
            ));
        }
        match bytes[i] {
            b'(' => {
                depth += 1;
                i += 1;
            }
            b')' => {
                depth -= 1;
                i += 1;
                if depth == 0 {
                    return Ok(i);
                }
            }
            b'"' => i = scan_str(bytes, source, i)?,
            _ => i += 1,
        }
    }
}

/// One segment of a split interpolated string (v0.43): literal text (escapes
/// resolved) or the absolute source span of a hole's expression (the bytes
/// between `\(` and its matching `)`). The parser turns the latter into a real
/// `Expr`; the lexer owns only the scanning.
pub(crate) enum InterpSegment {
    Chunk(String),
    Hole(Span),
}

/// Split an `InterpStr` token (its `span` covers the whole `"…"`) into chunks
/// and hole spans. Escapes in the chunks are resolved here (mirroring
/// [`parse_string_literal`]); holes are returned as spans for the parser to
/// re-lex and parse as expressions. (v0.43.)
pub(crate) fn split_interp(source: &str, span: Span) -> Result<Vec<InterpSegment>, CompileError> {
    let bytes = source.as_bytes();
    let inner_end = span.end - 1; // the closing `"`
    let mut segments = Vec::new();
    let mut chunk = String::new();
    let mut i = span.start + 1; // past the opening `"`
    while i < inner_end {
        match bytes[i] {
            b'\\' => match bytes[i + 1] {
                b'n' => {
                    chunk.push('\n');
                    i += 2;
                }
                b't' => {
                    chunk.push('\t');
                    i += 2;
                }
                b'"' => {
                    chunk.push('"');
                    i += 2;
                }
                b'\\' => {
                    chunk.push('\\');
                    i += 2;
                }
                b'(' => {
                    if !chunk.is_empty() {
                        segments.push(InterpSegment::Chunk(std::mem::take(&mut chunk)));
                    }
                    let hole_start = i + 2;
                    let after = scan_hole(bytes, source, hole_start)?;
                    // `after` is one past the matching `)`; the hole body is
                    // everything up to that `)`.
                    segments.push(InterpSegment::Hole(Span::new(hole_start, after - 1)));
                    i = after;
                }
                // The lexer already validated every escape, so nothing else
                // can appear here.
                other => unreachable!("unvalidated escape `\\{}` in InterpStr", other as char),
            },
            _ => {
                let ch = source[i..].chars().next().unwrap();
                chunk.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    if !chunk.is_empty() {
        segments.push(InterpSegment::Chunk(chunk));
    }
    Ok(segments)
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
            kinds("-> == != <= >= && || + - * / ! = < > ( ) { } [ ] , : . @"),
            vec![
                Arrow, EqEq, BangEq, LtEq, GtEq, AmpAmp, PipePipe, Plus, Minus, Star, Slash, Bang,
                Eq, Lt, Gt, LParen, RParen, LBrace, RBrace, LBracket, RBracket, Comma, Colon, Dot,
                At,
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
        assert_eq!(err.category, "bynk.lex.unterminated_string");
    }

    #[test]
    fn integer_overflow_is_error() {
        let err = tokenize("99999999999999999999").unwrap_err();
        assert_eq!(err.category, "bynk.lex.integer_overflow");
    }

    #[test]
    fn unexpected_character_is_error() {
        let err = tokenize("type X = Int $").unwrap_err();
        assert_eq!(err.category, "bynk.lex.unexpected_character");
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
    fn v0_7_keywords() {
        use TokenKind::*;
        assert_eq!(
            kinds("expect mocks suite case"),
            vec![Expect, Mocks, Suite, Case],
        );
    }

    #[test]
    fn fat_arrow_and_underscore() {
        use TokenKind::*;
        assert_eq!(kinds("_ =>"), vec![Underscore, FatArrow]);
    }

    // -- v0.43 string interpolation --

    #[test]
    fn interp_string_is_one_token() {
        use TokenKind::*;
        assert_eq!(kinds(r#""Hello, \(name)!""#), vec![InterpStr]);
        // A plain string (no hole) stays a `StrLit`, via the logos path.
        assert_eq!(kinds(r#""Hello, world""#), vec![StrLit]);
    }

    #[test]
    fn interp_balances_nested_parens_and_strings() {
        use TokenKind::*;
        // The `)` inside `f(x)` must not close the hole early.
        assert_eq!(kinds(r#""= \(f(x))""#), vec![InterpStr]);
        // A `)` inside a nested string inside the hole is also ignored.
        assert_eq!(kinds(r#""= \(label(")"))""#), vec![InterpStr]);
        // A nested interpolated string inside a hole.
        assert_eq!(kinds(r#""out \("in \(x)")""#), vec![InterpStr]);
    }

    #[test]
    fn escaped_open_paren_is_not_a_hole() {
        use TokenKind::*;
        // `\\(` is a literal backslash followed by `(` — no hole, so the
        // string lexes as a plain `StrLit` on the logos path.
        assert_eq!(kinds(r#""a \\(b) c""#), vec![StrLit]);
    }

    #[test]
    fn unterminated_hole_is_an_error() {
        // The hole runs to end of line without its closing `)`.
        let err = tokenize("\"value \\(x + 1\n\"").unwrap_err();
        assert_eq!(err.category, "bynk.lex.unterminated_interpolation");
    }

    #[test]
    fn unterminated_interp_string_is_an_error() {
        // A hole closes but the string never does (newline before the `"`).
        let err = tokenize("\"value \\(x) more\n").unwrap_err();
        assert_eq!(err.category, "bynk.lex.unterminated_string");
    }

    #[test]
    fn bad_escape_in_interp_string_is_an_error() {
        let err = tokenize(r#""a \q \(x)""#).unwrap_err();
        assert_eq!(err.category, "bynk.lex.bad_escape");
    }
}
