//! Hand-written recursive-descent parser for Bynk v0.
//!
//! Token grammar in spec §4. The expression parser uses one function per
//! precedence level (§4.4). Errors carry spans and short fix-oriented
//! messages; the parser does not currently attempt synchronisation, which
//! means at most one parse error is reported per compilation.

use crate::ast::*;
use crate::error::CompileError;
use crate::lexer::{Token, TokenKind, comment_body, doc_block_content, has_blank_line_between};
use crate::span::Span;
mod declarations;
mod expressions;
mod statements;
mod types;

/// Side-channel store for line-comment trivia (v1.1 LSP spec §3.5).
///
/// Built once up-front by [`split_trivia`] from the raw lexer token stream.
/// Comments are removed from the token stream the parser walks; their text
/// is filed into `leading` (comments on lines preceding a content token)
/// and `trailing` (a single comment on the same line as a content token).
/// The parser consumes entries through [`TriviaTable::take_leading`] and
/// [`TriviaTable::take_trailing`] as it recognises declarations.
#[derive(Debug, Default)]
struct TriviaTable {
    /// `leading[i]` holds the comment-body texts that appear immediately
    /// before content token `i` (zero or more `--` lines, in source order,
    /// not separated from the token by another content token).
    leading: Vec<Vec<String>>,
    /// `trailing[i]` holds an optional comment on the same source line as
    /// content token `i`. Only one trailing comment is recorded per token
    /// because a single `--` consumes the rest of the line.
    trailing: Vec<Option<String>>,
    /// Any pending leading comments at end-of-file (no content token
    /// followed). Used to preserve file-trailing comments.
    epilogue: Vec<String>,
}

impl TriviaTable {
    fn take_leading(&mut self, index: usize) -> Vec<String> {
        match self.leading.get_mut(index) {
            Some(v) => std::mem::take(v),
            None => Vec::new(),
        }
    }

    fn take_trailing(&mut self, index: usize) -> Option<String> {
        self.trailing.get_mut(index).and_then(|s| s.take())
    }

    fn take_epilogue(&mut self) -> Vec<String> {
        std::mem::take(&mut self.epilogue)
    }
}

/// Remove `Comment` trivia tokens from `tokens` and bin them into a
/// [`TriviaTable`] keyed against the surviving content tokens. A comment
/// on the same source line as the preceding content token is recorded as
/// that token's *trailing* trivia; everything else is *leading* for the
/// next content token.
fn split_trivia(tokens: &[Token], source: &str) -> (Vec<Token>, TriviaTable) {
    let mut filtered: Vec<Token> = Vec::with_capacity(tokens.len());
    let mut table = TriviaTable::default();
    let mut pending_leading: Vec<String> = Vec::new();
    let mut last_content_end: Option<usize> = None;
    for tok in tokens {
        if tok.kind == TokenKind::Comment {
            let body = comment_body(source, tok.span).to_string();
            // If nothing has been buffered as leading for the next token and
            // there is no newline between the previous content token and
            // this comment, it trails that token.
            if pending_leading.is_empty()
                && let Some(prev_end) = last_content_end
                && !source[prev_end..tok.span.start].contains('\n')
            {
                let last_idx = filtered.len() - 1;
                // Only attach if no trailing already recorded (shouldn't
                // happen because `--` consumes through end-of-line).
                if table.trailing[last_idx].is_none() {
                    table.trailing[last_idx] = Some(body);
                    continue;
                }
            }
            pending_leading.push(body);
            continue;
        }
        filtered.push(*tok);
        table.leading.push(std::mem::take(&mut pending_leading));
        table.trailing.push(None);
        last_content_end = Some(tok.span.end);
    }
    table.epilogue = pending_leading;
    (filtered, table)
}

/// Parse a token slice into a [`Commons`] AST.
///
/// Accepts either form of v0.3 commons file:
/// - Brace form: `commons name { items... }` (v0–v0.2 compatible).
/// - Fragment form: `commons name uses... items...` to EOF (v0.3).
pub fn parse(tokens: &[Token], source: &str) -> Result<Commons, Vec<CompileError>> {
    parse_with_warnings(tokens, source).map(|(c, _warnings)| c)
}

/// [`parse`] with the non-fatal diagnostics threaded out alongside the AST
/// (ADR 0117) — see [`parse_units_with_warnings`].
pub fn parse_with_warnings(
    tokens: &[Token],
    source: &str,
) -> Result<(Commons, Vec<CompileError>), Vec<CompileError>> {
    let (unit, warnings) = parse_unit_with_warnings(tokens, source)?;
    match unit {
        SourceUnit::Commons(c) => Ok((c, warnings)),
        SourceUnit::Context(ctx) => Err(vec![
            CompileError::new(
                "bynk.parse.unexpected_context",
                ctx.span,
                "expected a `commons` declaration but found a `context` declaration",
            )
            .with_note(
                "contexts must be compiled as part of a project — pass the source directory, e.g. `bynkc compile --target bundle --output out src`",
            ),
        ]),
        SourceUnit::Suite(t) => Err(vec![
            CompileError::new(
                "bynk.parse.unexpected_suite",
                t.span,
                "expected a `commons` declaration but found a `suite` declaration",
            )
            .with_note(
                "tests must be compiled as part of a project — pass the source directory, e.g. `bynkc compile --target bundle --output out src`",
            ),
        ]),
        SourceUnit::Adapter(a) => Err(vec![
            CompileError::new(
                "bynk.parse.unexpected_adapter",
                a.span,
                "expected a `commons` declaration but found an `adapter` declaration",
            )
            .with_note(
                "adapters must be compiled as part of a project — pass the source directory, e.g. `bynkc compile --target bundle --output out src`",
            ),
        ]),
    }
}

/// Parse a token slice into a [`SourceUnit`] with error recovery, returning a
/// best-effort partial AST plus the full list of parse errors and warnings.
///
/// Used by the LSP: item-level recovery skips past a malformed declaration to
/// the next top-level item, so multiple errors are reported per compilation
/// rather than just the first. Compared to [`parse_unit`], this never bails;
/// if no SourceUnit could be parsed at all (e.g. the file is empty or the
/// header itself fails) the returned `Option` is `None`.
pub fn parse_unit_with_recovery(
    tokens: &[Token],
    source: &str,
) -> (Option<SourceUnit>, Vec<CompileError>) {
    let (filtered, trivia) = split_trivia(tokens, source);
    let mut warnings = Vec::new();
    let mut p = Parser::new(&filtered, source, trivia, &mut warnings);
    p.recover_mode = true;
    let unit_opt = match p.parse_unit() {
        Ok(u) => {
            // v0.113: a file may hold more than one top-level unit (an atomic
            // `commons` + `suite` file, DECISION S). Consume any further units
            // so trailing declarations are not mis-reported as stray tokens; the
            // editor view is keyed on the first (primary) unit. A genuinely
            // malformed trailing declaration is still surfaced via recovery.
            while p.peek().is_some() {
                match p.parse_unit() {
                    Ok(_) => {}
                    Err(e) => {
                        p.recovered_errors.push(e);
                        break;
                    }
                }
            }
            Some(u)
        }
        Err(e) => {
            p.recovered_errors.push(e);
            None
        }
    };
    let mut all_errors = p.recovered_errors;
    all_errors.append(&mut warnings);
    (unit_opt, all_errors)
}

/// Parse a token slice into a [`SourceUnit`] — either a commons or a context.
///
/// Each `.bynk` file is exactly one declaration of one kind.
pub fn parse_unit(tokens: &[Token], source: &str) -> Result<SourceUnit, Vec<CompileError>> {
    parse_unit_with_warnings(tokens, source).map(|(unit, _warnings)| unit)
}

/// [`parse_unit`] with the non-fatal diagnostics threaded out alongside the
/// AST (ADR 0117) — see [`parse_units_with_warnings`].
pub fn parse_unit_with_warnings(
    tokens: &[Token],
    source: &str,
) -> Result<(SourceUnit, Vec<CompileError>), Vec<CompileError>> {
    let (filtered, trivia) = split_trivia(tokens, source);
    let mut warnings = Vec::new();
    let mut p = Parser::new(&filtered, source, trivia, &mut warnings);
    let result = match p.parse_unit() {
        Ok(u) => {
            if let Some(extra) = p.peek() {
                Err(vec![
                    CompileError::new(
                        "bynk.parse.extra_tokens",
                        extra.span,
                        "unexpected token after top-level declaration",
                    )
                    .with_note(
                        "a `.bynk` file contains exactly one `commons` or `context` declaration",
                    ),
                ])
            } else {
                Ok(u)
            }
        }
        Err(e) => Err(vec![e]),
    };
    // ADR 0117: warnings (e.g. orphan doc blocks) ride alongside a successful
    // parse — severity governs gating at the caller, not here.
    match result {
        Ok(u) => Ok((u, warnings)),
        Err(mut errs) => {
            errs.append(&mut warnings);
            Err(errs)
        }
    }
}

/// Parse a token slice into **all** the top-level [`SourceUnit`]s in one file
/// (v0.113, testing track slice 1b). A `.bynk` file may hold more than one
/// top-level declaration — an *atomic* file with `commons`/`context` **and** a
/// `suite` together (DECISION S) — so the compiler parses a `Vec`, not a single
/// unit. Test-ness is a property of each declaration, not of the file.
///
/// Bails on the first malformed declaration (like [`parse_unit`], not the
/// recovering LSP path). An empty file is an error.
pub fn parse_units(tokens: &[Token], source: &str) -> Result<Vec<SourceUnit>, Vec<CompileError>> {
    parse_units_with_warnings(tokens, source).map(|(units, _warnings)| units)
}

/// [`parse_units`] with the non-fatal diagnostics threaded out alongside the
/// AST (ADR 0117): a successful parse returns `Ok((units, warnings))` instead
/// of hard-failing on a warning-severity diagnostic (an orphan doc block used
/// to abort file discovery and throw the good AST away). A failed parse still
/// returns every diagnostic — errors then warnings — in the `Err`.
pub fn parse_units_with_warnings(
    tokens: &[Token],
    source: &str,
) -> Result<(Vec<SourceUnit>, Vec<CompileError>), Vec<CompileError>> {
    let (filtered, trivia) = split_trivia(tokens, source);
    let mut warnings = Vec::new();
    let mut p = Parser::new(&filtered, source, trivia, &mut warnings);
    let mut units = Vec::new();
    let mut errors: Vec<CompileError> = Vec::new();
    while p.peek().is_some() {
        match p.parse_unit() {
            Ok(u) => units.push(u),
            Err(e) => {
                errors.push(e);
                break;
            }
        }
    }
    let eof = p.eof_span();
    // `p` (and thus its `&mut warnings` borrow) is no longer used past here, so
    // the local `warnings` are readable again.
    if !errors.is_empty() {
        errors.append(&mut warnings);
        return Err(errors);
    }
    if units.is_empty() {
        return Err(vec![CompileError::new(
            "bynk.parse.unexpected_eof",
            eof,
            "expected `commons`, `context`, or `suite` to start the file, found end of file",
        )]);
    }
    Ok((units, warnings))
}

/// A signed numeric literal in refinement-bound position (v0.21): `InRange`
/// bounds are either both `Int` or both `Float`.
enum SignedNumLit {
    Int(IntBound),
    Float(FloatBound),
}

struct Parser<'a> {
    tokens: &'a [Token],
    source: &'a str,
    pos: usize,
    /// Accumulated non-fatal diagnostics. v0.3 uses this for orphan-doc
    /// warnings, which are emitted as errors with a distinguishable category.
    warnings: &'a mut Vec<CompileError>,
    /// When true, the item-level loops catch errors from individual item
    /// parses, push them into `recovered_errors`, and skip forward to the
    /// next top-level item boundary instead of bailing. Used by the LSP via
    /// [`parse_unit_with_recovery`]; disabled in the normal `parse` path so
    /// existing single-error behaviour is preserved.
    recover_mode: bool,
    /// Errors collected during recovery-mode parsing. Only populated when
    /// `recover_mode` is true.
    recovered_errors: Vec<CompileError>,
    /// Line-comment trivia separated from the token stream. See
    /// [`TriviaTable`].
    trivia: TriviaTable,
    /// Live recursion depth of the three self-recursive parse entry points
    /// (`parse_expr`, `parse_type_ref`, `parse_pattern`). Incremented on entry
    /// and decremented on exit by [`Parser::enter_recursion`] so it tracks the
    /// current stack depth; when it exceeds [`crate::MAX_NESTING_DEPTH`] the
    /// parser reports a bounded-depth diagnostic instead of overflowing its
    /// stack (#713).
    depth: usize,
    /// When true, a bare `ident {` on the *spine* of the current expression is
    /// an identifier followed by an unrelated block, never a record
    /// construction — so an `if`/`match` condition that ends in a bare
    /// identifier does not swallow the branch/arm block as `Ident { field }`
    /// (#636). Set only around the condition parse (see [`parse_cond_expr`]);
    /// `parse_expr` clears it, so the restriction is lifted inside any
    /// delimited sub-expression (parentheses, call arguments, list, record
    /// field). Mirrors Rust's `NO_STRUCT_LITERAL` restriction.
    no_record_literal: bool,
}

impl<'a> Parser<'a> {
    fn new(
        tokens: &'a [Token],
        source: &'a str,
        trivia: TriviaTable,
        warnings: &'a mut Vec<CompileError>,
    ) -> Self {
        Self {
            tokens,
            source,
            pos: 0,
            warnings,
            recover_mode: false,
            recovered_errors: Vec::new(),
            trivia,
            depth: 0,
            no_record_literal: false,
        }
    }

    /// Enter a self-recursive parse step, bumping the live recursion depth and
    /// failing with a bounded-depth diagnostic if it would exceed
    /// [`crate::MAX_NESTING_DEPTH`]. The caller pairs a successful entry with a
    /// matching `self.depth -= 1` on the way out (see `parse_expr` /
    /// `parse_type_ref`); on the error path the depth is restored here so a
    /// recovering caller is not left mis-counted. `what` names the construct
    /// for the message (e.g. "this expression", "this type"). See #713.
    fn enter_recursion(&mut self, what: &str) -> Result<(), CompileError> {
        self.depth += 1;
        if self.depth > crate::MAX_NESTING_DEPTH {
            self.depth -= 1;
            let span = self
                .peek()
                .map(|t| t.span)
                .unwrap_or_else(|| self.eof_span());
            return Err(CompileError::new(
                "bynk.parse.nesting_too_deep",
                span,
                format!(
                    "{what} nests more than {} levels deep",
                    crate::MAX_NESTING_DEPTH
                ),
            )
            .with_note(
                "deeply nested source is rejected to keep the parser from overflowing its \
                 stack and aborting; flatten or split the construct",
            ));
        }
        Ok(())
    }

    /// Comments immediately preceding the current peek position. Consumed
    /// (the table entry is cleared) so the same comments are not attached
    /// to two nodes.
    fn take_leading_trivia(&mut self) -> Vec<String> {
        self.trivia.take_leading(self.pos)
    }

    /// Trailing comment, if any, on the same source line as the most
    /// recently consumed content token. Call AFTER finishing a declaration
    /// or statement, while `self.pos` points one past its last token.
    fn take_trailing_trivia(&mut self) -> Option<String> {
        if self.pos == 0 {
            return None;
        }
        self.trivia.take_trailing(self.pos - 1)
    }

    /// Handle a per-item parse error. In recovery mode, record the error and
    /// advance to the next sync point so the item loop can continue; otherwise
    /// propagate as a hard failure.
    fn handle_item_err(&mut self, e: CompileError) -> Result<(), CompileError> {
        if self.recover_mode {
            self.recovered_errors.push(e);
            let before = self.pos;
            self.recover_to_top_item();
            // The sync target may be the very token that produced the error —
            // a context-only keyword (`capability`, `service`, …) at item
            // position in a commons errors *without consuming it*, and it is
            // itself a sync point. Recovery must always make progress, or the
            // item loop re-reports the same error until memory runs out
            // (found by the `parse` fuzz target on a seed input).
            if self.pos == before {
                self.bump();
            }
            Ok(())
        } else {
            Err(e)
        }
    }

    /// Skip forward to the next top-level item boundary: either a top-level
    /// declaration keyword (`type`, `fn`, `uses`, `consumes`, `exports`,
    /// `capability`, `provides`, `service`, `agent`), a closing brace, or
    /// end-of-input. Used only in recovery mode.
    fn recover_to_top_item(&mut self) {
        while let Some(t) = self.peek() {
            match t.kind {
                TokenKind::Type
                | TokenKind::Fn
                | TokenKind::Uses
                | TokenKind::Consumes
                | TokenKind::Exports
                | TokenKind::Capability
                | TokenKind::Provides
                | TokenKind::Stub
                | TokenKind::Service
                | TokenKind::Agent
                | TokenKind::Suite
                | TokenKind::Case
                | TokenKind::RBrace
                | TokenKind::Commons
                | TokenKind::Context => return,
                _ => {
                    self.bump();
                }
            }
        }
    }

    fn peek(&self) -> Option<Token> {
        self.tokens.get(self.pos).copied()
    }

    fn peek_kind(&self) -> Option<TokenKind> {
        self.peek().map(|t| t.kind)
    }

    /// The token `n` positions ahead of the cursor (`nth(0)` == `peek()`).
    fn nth(&self, n: usize) -> Option<Token> {
        self.tokens.get(self.pos + n).copied()
    }

    fn nth_kind(&self, n: usize) -> Option<TokenKind> {
        self.nth(n).map(|t| t.kind)
    }

    /// The source text of the token `n` positions ahead, or `""` if none.
    fn nth_text(&self, n: usize) -> &'a str {
        self.nth(n).map(|t| self.slice(t.span)).unwrap_or("")
    }

    /// The span of the most recently consumed token (`self.pos - 1`). Falls back
    /// to the current token's span when nothing has been consumed yet.
    fn prev_span(&self) -> Span {
        self.tokens
            .get(self.pos.wrapping_sub(1))
            .or_else(|| self.peek_ref())
            .map(|t| t.span)
            .unwrap_or_default()
    }

    fn peek_ref(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn bump(&mut self) -> Option<Token> {
        let t = self.peek();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn eat(&mut self, kind: TokenKind) -> Option<Token> {
        if self.peek_kind() == Some(kind) {
            self.bump()
        } else {
            None
        }
    }

    fn slice(&self, span: Span) -> &'a str {
        &self.source[span.range()]
    }

    /// True when the next token sits on a later line than `prev`. Used to
    /// keep a `[` that opens a new line out of the postfix type-application
    /// form: `f` followed by `[1, 2]` on the next line is an identifier and
    /// a list literal, not `f[…]` (v0.20b).
    fn next_token_on_new_line(&self, prev: Span) -> bool {
        match self.peek() {
            Some(t) if prev.end <= t.span.start => {
                self.source[prev.end..t.span.start].contains('\n')
            }
            _ => false,
        }
    }

    /// Span pointing at the end of input — used for "unexpected EOF" reports.
    /// The start backs up to the **start of the final char**, not `len - 1`, so
    /// the span never splits a multibyte codepoint (an unterminated construct
    /// whose last line ends in non-ASCII — e.g. a `--` comment ending in `→`).
    fn eof_span(&self) -> Span {
        let end = self.source.len();
        let start = (0..end)
            .rev()
            .find(|&i| self.source.is_char_boundary(i))
            .unwrap_or(0);
        Span::new(start, end)
    }

    fn expect(&mut self, kind: TokenKind, ctx: &str) -> Result<Token, CompileError> {
        match self.peek() {
            Some(t) if t.kind == kind => {
                self.bump();
                Ok(t)
            }
            Some(t) => Err(CompileError::new(
                "bynk.parse.expected_token",
                t.span,
                format!(
                    "expected {} {ctx}, found {}",
                    kind.describe(),
                    t.kind.describe()
                ),
            )),
            None => Err(CompileError::new(
                "bynk.parse.unexpected_eof",
                self.eof_span(),
                format!("expected {} {ctx}, found end of file", kind.describe()),
            )),
        }
    }

    fn expect_ident(&mut self, ctx: &str) -> Result<Ident, CompileError> {
        match self.peek() {
            Some(t) if t.kind == TokenKind::Ident => {
                self.bump();
                Ok(Ident {
                    name: self.slice(t.span).to_string(),
                    span: t.span,
                })
            }
            // v0.5 contextual keyword `on` doubles as an identifier in
            // expression / field-access positions so users can name fields and
            // parameters using it. It retains its keyword meaning only at
            // handler-decl-level (`on call(...)`).
            //
            // v0.7 / v0.112: `suite` and `case` are contextual too — they
            // introduce the suite declaration and its cases, but are perfectly
            // valid commons/context/field names otherwise.
            Some(t) if matches!(t.kind, TokenKind::On | TokenKind::Suite | TokenKind::Case) => {
                self.bump();
                Ok(Ident {
                    name: self.slice(t.span).to_string(),
                    span: t.span,
                })
            }
            Some(t) if is_reserved_keyword(t.kind) => Err(CompileError::new(
                "bynk.parse.reserved_keyword",
                t.span,
                format!(
                    "expected identifier {ctx}, but `{}` is a reserved keyword",
                    self.slice(t.span)
                ),
            )
            .with_note("rename the identifier to something that is not a keyword")),
            Some(t) => Err(CompileError::new(
                "bynk.parse.expected_token",
                t.span,
                format!("expected identifier {ctx}, found {}", t.kind.describe()),
            )),
            None => Err(CompileError::new(
                "bynk.parse.unexpected_eof",
                self.eof_span(),
                format!("expected identifier {ctx}, found end of file"),
            )),
        }
    }

    // -- top level --

    /// Consume an optional doc block at the current position, returning the
    /// (content, end-of-doc span) pair. Returns None if the next token is not
    /// a doc block.
    fn take_doc_block(&mut self) -> Option<(String, Span)> {
        if self.peek_kind() == Some(TokenKind::DocBlock) {
            let t = self.bump().unwrap();
            let body = doc_block_content(self.source, t.span);
            return Some((body, t.span));
        }
        None
    }

    /// Collect all line-comment trivia leading the next declaration plus
    /// the optional doc block. Comments may appear both *before* and
    /// *between* the doc and the declaration; the spec canonicalises both
    /// groups above the doc, so we concatenate them.
    fn collect_item_lead(&mut self) -> (Vec<String>, Option<(String, Span)>) {
        let mut leading = self.take_leading_trivia();
        let doc = self.take_doc_block();
        if doc.is_some() {
            leading.extend(self.take_leading_trivia());
        }
        (leading, doc)
    }

    /// Attach a parsed doc block to a following declaration unless a blank
    /// line separates them, in which case the doc is orphaned (warning).
    fn finalize_doc(&mut self, doc: Option<(String, Span)>, next_span: Span) -> Option<String> {
        let (content, doc_span) = doc?;
        // A blank line between the doc and the next decl orphans the doc.
        if has_blank_line_between(self.source, doc_span.end, next_span.start) {
            self.warnings.push(
                CompileError::new(
                    "bynk.parse.orphan_doc_block",
                    doc_span,
                    "documentation block is separated from the following declaration by a blank line; it will not be attached",
                )
                .with_note(
                    "remove the blank line to attach the doc to the next declaration, \
                     or remove the doc block if it is not meant to document anything",
                ),
            );
            return None;
        }
        Some(content)
    }
}

/// Parse the body of a lexed double-quoted string literal (the lexeme,
/// including surrounding quotes), applying the v0 escape rules.
fn parse_string_literal(lexeme: &str, span: Span) -> Result<String, CompileError> {
    let bytes = lexeme.as_bytes();
    debug_assert!(bytes.first() == Some(&b'"') && bytes.last() == Some(&b'"'));
    let inner = &lexeme[1..lexeme.len() - 1];
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                other => {
                    return Err(CompileError::new(
                        "bynk.lex.bad_escape",
                        span,
                        format!(
                            "invalid escape sequence `\\{}` in string literal",
                            other.map(|c| c.to_string()).unwrap_or_default()
                        ),
                    )
                    .with_note("supported escapes: \\n \\t \\\" \\\\"));
                }
            }
        } else {
            out.push(c);
        }
    }
    Ok(out)
}

fn is_reserved_keyword(kind: TokenKind) -> bool {
    use TokenKind::*;
    matches!(
        kind,
        Commons
            | Type
            | Fn
            | Where
            | True
            | False
            | Int
            | String
            | Bool
            | Let
            | If
            | Else
            | Ok
            | Err
            | Result
            | ValidationError
            | Enum
            | Match
            | Option
            | Record
            | Self_
            | Some
            | None
            | Is
            | Opaque
            | Uses
            | Context
            | Consumes
            | Exports
            | Transparent
            | Agent
            | As
            | Capability
            | Effect
            | Do
            | Given
            | On
            | Http
            | Provides
            | Stub
            | Service
            | Actor
            | By
            | Expect
            | Suite
            | Case
            | Float
            | Duration
            | Instant
            | Bytes
            | JsonError
            | Property
            | Adapter
            | Binding
            | Cron
            | Queue
            | From
            | Protocol
            | Invariant
            | Implies
            | Requires
            | Ensures
            | Transition
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;

    fn parse_str(src: &str) -> Result<Commons, Vec<CompileError>> {
        let toks = tokenize(src).map_err(|e| vec![e])?;
        parse(&toks, src)
    }

    fn parse_recover_str(src: &str) -> (Option<SourceUnit>, Vec<CompileError>) {
        let toks = match tokenize(src) {
            Ok(t) => t,
            Err(e) => return (None, vec![e]),
        };
        parse_unit_with_recovery(&toks, src)
    }

    #[test]
    fn eof_span_never_splits_a_multibyte_codepoint() {
        // An unterminated construct whose final line ends in a non-ASCII char
        // (here a `--` comment ending in `→`) once produced an `unexpected_eof`
        // span of `len - 1 .. len`, landing on the arrow's last continuation
        // byte. Every reported span must sit on char boundaries.
        for src in [
            "commons x {\n  -- ends with an arrow →",
            "agent A {\n  key k: String\n  -- note 🦀",
            "commons y {\n  type T = é",
        ] {
            let (_unit, errors) = parse_recover_str(src);
            for e in &errors {
                assert!(
                    src.is_char_boundary(e.span.start) && src.is_char_boundary(e.span.end),
                    "span {:?} splits a codepoint in {src:?}",
                    e.span,
                );
            }
        }
    }

    #[test]
    fn recovery_skips_garbage_between_decls() {
        // Two `type` declarations separated by garbage. Recovery should
        // accept both and report one error for the garbage between them.
        let src = "commons x {\n\
                   type A = Int where NonNegative\n\
                   ??? !!!\n\
                   type B = String where NonEmpty\n\
                   }";
        let (unit, errors) = parse_recover_str(src);
        let unit = unit.expect("recovery should produce a partial AST");
        let SourceUnit::Commons(c) = unit else {
            panic!("expected commons")
        };
        // Both type decls should have been collected despite the garbage.
        let names: Vec<_> = c
            .items
            .iter()
            .map(|i| match i {
                CommonsItem::Type(t) => t.name.name.clone(),
                _ => panic!("expected only types"),
            })
            .collect();
        assert!(
            names.contains(&"A".to_string()) && names.contains(&"B".to_string()),
            "expected both A and B; got {names:?}",
        );
        assert!(!errors.is_empty(), "expected at least one parse error");
    }

    #[test]
    fn recovery_handles_bad_first_decl_then_good_second() {
        // First decl is malformed (missing `=`); second is well-formed.
        let src = "commons x {\n\
                   type A Int where NonNegative\n\
                   type B = String where NonEmpty\n\
                   }";
        let (unit, errors) = parse_recover_str(src);
        let unit = unit.expect("recovery should produce a partial AST");
        let SourceUnit::Commons(c) = unit else {
            panic!("expected commons")
        };
        let names: Vec<_> = c
            .items
            .iter()
            .filter_map(|i| match i {
                CommonsItem::Type(t) => Some(t.name.name.clone()),
                _ => None,
            })
            .collect();
        assert!(
            names.contains(&"B".to_string()),
            "B should be parsed after A's failure; got {names:?}"
        );
        assert!(!errors.is_empty(), "expected at least one parse error");
    }

    #[test]
    fn doc_block_attaches_to_type() {
        let c =
            parse_str("commons x {\n---\nA descriptive doc.\n---\ntype T = Int where Positive\n}")
                .unwrap();
        let CommonsItem::Type(t) = &c.items[0] else {
            panic!()
        };
        assert!(t.documentation.is_some());
        assert!(
            t.documentation
                .as_ref()
                .unwrap()
                .contains("A descriptive doc.")
        );
    }

    #[test]
    fn interpolated_string_parses_into_parts() {
        // v0.43: `"Hi, \(name)!"` splits into chunk / hole / chunk.
        let c = parse_str("commons x\n\nfn f(name: String) -> String {\n  \"Hi, \\(name)!\"\n}\n")
            .unwrap();
        let CommonsItem::Fn(f) = &c.items[0] else {
            panic!("expected fn")
        };
        let ExprKind::InterpStr(parts) = &f.body.tail.kind else {
            panic!("expected InterpStr, got {:?}", f.body.tail.kind)
        };
        assert_eq!(parts.len(), 3);
        assert!(matches!(&parts[0], InterpPart::Chunk(s) if s == "Hi, "));
        assert!(
            matches!(&parts[1], InterpPart::Hole(h) if matches!(&h.kind, ExprKind::Ident(id) if id.name == "name"))
        );
        assert!(matches!(&parts[2], InterpPart::Chunk(s) if s == "!"));
    }

    #[test]
    fn interpolated_hole_parses_a_full_expression() {
        // A hole holds an arbitrary expression, not just an identifier.
        let c =
            parse_str("commons x\n\nfn f(a: Int, b: Int) -> String {\n  \"sum = \\(a + b)\"\n}\n")
                .unwrap();
        let CommonsItem::Fn(f) = &c.items[0] else {
            panic!("expected fn")
        };
        let ExprKind::InterpStr(parts) = &f.body.tail.kind else {
            panic!("expected InterpStr")
        };
        assert!(matches!(&parts[1], InterpPart::Hole(h) if matches!(&h.kind, ExprKind::BinOp(..))));
    }

    #[test]
    fn empty_interpolation_hole_is_rejected() {
        let errs = parse_str("commons x\n\nfn f() -> String {\n  \"\\()\"\n}\n").unwrap_err();
        assert!(
            errs.iter()
                .any(|e| e.category == "bynk.parse.empty_interpolation"),
            "expected empty_interpolation; got {errs:?}"
        );
    }

    #[test]
    fn fragment_form_parses() {
        let c = parse_str("commons x.y\n\ntype T = Int where NonNegative\n").unwrap();
        assert_eq!(c.form, CommonsForm::Fragment);
        assert_eq!(c.items.len(), 1);
    }

    #[test]
    fn uses_parses() {
        let c = parse_str("commons x\n\nuses other.lib\n").unwrap();
        assert_eq!(c.uses.len(), 1);
        assert_eq!(c.uses[0].target.joined(), "other.lib");
    }

    fn parse_unit_str(src: &str) -> Result<SourceUnit, Vec<CompileError>> {
        let toks = tokenize(src).map_err(|e| vec![e])?;
        parse_unit(&toks, src)
    }

    #[test]
    fn minimal_context_parses() {
        let u = parse_unit_str("context commerce.orders {}").unwrap();
        let SourceUnit::Context(c) = u else {
            panic!("expected context");
        };
        assert_eq!(c.name.joined(), "commerce.orders");
        assert!(c.items.is_empty());
    }

    #[test]
    fn context_consumes_and_exports_parse() {
        let src = "context commerce.orders {\n  uses commerce.money\n  consumes commerce.payment\n  exports opaque { OrderId }\n  exports transparent { OrderError }\n  type OrderId = String where Matches(\"ORD-[0-9]+\")\n  type OrderError = enum { CartEmpty, BadInput }\n}";
        let u = parse_unit_str(src).unwrap();
        let SourceUnit::Context(c) = u else { panic!() };
        assert_eq!(c.uses.len(), 1);
        assert_eq!(c.consumes.len(), 1);
        assert_eq!(c.exports.len(), 2);
        assert_eq!(c.exports[0].kind, ExportKind::Type(Visibility::Opaque));
        assert_eq!(c.exports[1].kind, ExportKind::Type(Visibility::Transparent));
    }

    #[test]
    fn context_fragment_form_parses() {
        let src = "context x.y\n\nuses other.lib\nconsumes other.ctx\nexports opaque { T }\n\ntype T = Int where NonNegative\n";
        let u = parse_unit_str(src).unwrap();
        let SourceUnit::Context(c) = u else { panic!() };
        assert_eq!(c.form, CommonsForm::Fragment);
        assert_eq!(c.uses.len(), 1);
        assert_eq!(c.consumes.len(), 1);
        assert_eq!(c.exports.len(), 1);
    }

    #[test]
    fn opaque_type_parses() {
        let c = parse_str("commons x { type T = opaque Int where NonNegative }").unwrap();
        let CommonsItem::Type(t) = &c.items[0] else {
            panic!()
        };
        assert!(matches!(t.body, TypeBody::Opaque { .. }));
    }

    #[test]
    fn empty_commons() {
        let c = parse_str("commons fitness.units {}").unwrap();
        assert_eq!(c.name.joined(), "fitness.units");
        assert!(c.items.is_empty());
    }

    #[test]
    fn one_type_decl() {
        let c = parse_str("commons x { type Metres = Int where NonNegative }").unwrap();
        assert_eq!(c.items.len(), 1);
        let CommonsItem::Type(t) = &c.items[0] else {
            panic!()
        };
        assert_eq!(t.name.name, "Metres");
        match &t.body {
            TypeBody::Refined {
                base, refinement, ..
            } => {
                assert_eq!(*base, BaseType::Int);
                assert!(refinement.is_some());
            }
            _ => panic!("expected refined body"),
        }
    }

    #[test]
    fn function_decl() {
        let c = parse_str("commons x { fn add(a: Int, b: Int) -> Int { a + b } }").unwrap();
        let CommonsItem::Fn(f) = &c.items[0] else {
            panic!()
        };
        assert_eq!(f.name.ident().name, "add");
        assert_eq!(f.params.len(), 2);
    }

    #[test]
    fn chained_comparison_is_error() {
        let errs = parse_str("commons x { fn f(a: Int, b: Int, c: Int) -> Bool { a < b < c } }")
            .unwrap_err();
        assert_eq!(errs[0].category, "bynk.parse.non_associative");
    }

    #[test]
    fn chained_equality_is_error() {
        let errs = parse_str("commons x { fn f(a: Int, b: Int, c: Int) -> Bool { a == b == c } }")
            .unwrap_err();
        assert_eq!(errs[0].category, "bynk.parse.non_associative");
    }

    /// Run `f` on a thread with a generous stack. The depth-guard tests build
    /// source that, *without* the guard, overflows — so if the guard ever
    /// regressed we want a clean assertion failure, not a `SIGABRT` that takes
    /// the whole test binary down. A large stack also absorbs the fat frames a
    /// debug build spends per recursion level (production release frames are
    /// ~9 KB/level, so `MAX_NESTING_DEPTH = 64` sits well inside a 1 MB stack;
    /// a debug frame is several times larger and would overflow libtest's
    /// default 2 MB test thread near the limit even though the guard fires).
    fn on_big_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
        std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn(f)
            .unwrap()
            .join()
            .unwrap()
    }

    #[test]
    fn deeply_nested_parens_are_bounded_not_overflowed() {
        // Without a depth guard the parenthesised-expression recursion
        // (`parse_primary` -> `parse_expr` -> …) overflows the stack and aborts
        // the process (#713). Well past the limit it must instead report a
        // bounded-depth diagnostic. The nesting is left open so the guard, not
        // a later `)`, is what stops the descent.
        let errs = on_big_stack(|| {
            let depth = crate::MAX_NESTING_DEPTH + 8;
            let src = format!(
                "commons x {{ fn f() -> Int {{ {}0{} }} }}",
                "(".repeat(depth),
                ")".repeat(depth),
            );
            parse_str(&src).unwrap_err()
        });
        assert_eq!(errs[0].category, "bynk.parse.nesting_too_deep");
    }

    #[test]
    fn deeply_nested_types_are_bounded_not_overflowed() {
        // The type parser self-recurses through generic type arguments
        // (`parse_type_ref` -> `parse_type_atom` -> `parse_type_ref`); the same
        // guard bounds it (#713). A right-nested `Result[Int, …]` in parameter
        // position drives that recursion.
        let errs = on_big_stack(|| {
            let depth = crate::MAX_NESTING_DEPTH + 8;
            let src = format!(
                "commons x {{ fn f(x: {}Int{}) -> Int {{ 0 }} }}",
                "Result[Int, ".repeat(depth),
                "]".repeat(depth),
            );
            parse_str(&src).unwrap_err()
        });
        assert_eq!(errs[0].category, "bynk.parse.nesting_too_deep");
    }

    #[test]
    fn deeply_nested_patterns_are_bounded_not_overflowed() {
        // Variant patterns are a third self-recursive descent (`parse_pattern`
        // -> `parse_pattern_binding` -> `parse_pattern`) that routes through
        // neither `parse_expr` nor `parse_type_ref`; without its own guard a
        // nested `Ok(Ok(…))` match arm reproduces the #713 crash.
        let errs = on_big_stack(|| {
            let depth = crate::MAX_NESTING_DEPTH + 8;
            let src = format!(
                "commons x {{ fn f(n: Int) -> Int {{ match n {{ {}n{} => 0 }} }} }}",
                "Ok(".repeat(depth),
                ")".repeat(depth),
            );
            parse_str(&src).unwrap_err()
        });
        assert_eq!(errs[0].category, "bynk.parse.nesting_too_deep");
    }

    #[test]
    fn nesting_below_the_limit_still_parses() {
        // The guard must not reject ordinary well-nested source: a paren-nested
        // expression comfortably under the limit still parses cleanly.
        let ok = on_big_stack(|| {
            let depth = crate::MAX_NESTING_DEPTH - 8;
            let src = format!(
                "commons x {{ fn f() -> Int {{ {}0{} }} }}",
                "(".repeat(depth),
                ")".repeat(depth),
            );
            parse_str(&src).is_ok()
        });
        assert!(ok, "well-nested source under the limit should parse");
    }

    #[test]
    fn let_statement_parses() {
        let c = parse_str("commons x { fn f(n: Int) -> Int { let y = n + 1\n y } }").unwrap();
        let CommonsItem::Fn(f) = &c.items[0] else {
            panic!()
        };
        assert_eq!(f.body.statements.len(), 1);
        match &f.body.statements[0] {
            Statement::Let(l) => {
                assert_eq!(l.name.name, "y");
                assert!(l.type_annot.is_none());
            }
            _ => panic!("expected a pure `let` statement"),
        }
    }

    #[test]
    fn let_with_annotation() {
        let c = parse_str("commons x { fn f(n: Int) -> Int { let y: Int = n\n y } }").unwrap();
        let CommonsItem::Fn(f) = &c.items[0] else {
            panic!()
        };
        match &f.body.statements[0] {
            Statement::Let(l) => assert!(l.type_annot.is_some()),
            _ => panic!("expected a pure `let` statement"),
        }
    }

    #[test]
    fn if_else_parses_as_expression() {
        let c = parse_str("commons x { fn f(b: Bool) -> Int { if b { 1 } else { 0 } } }").unwrap();
        let CommonsItem::Fn(f) = &c.items[0] else {
            panic!()
        };
        assert!(matches!(f.body.tail.kind, ExprKind::If { .. }));
    }

    #[test]
    fn else_if_chain_parses() {
        let c = parse_str(
            "commons x { fn f(n: Int) -> Int { if n < 0 { -1 } else if n == 0 { 0 } else { 1 } } }",
        )
        .unwrap();
        let CommonsItem::Fn(f) = &c.items[0] else {
            panic!()
        };
        let ExprKind::If { else_block, .. } = &f.body.tail.kind else {
            panic!()
        };
        // The else-branch is a block whose tail is another `If`.
        assert!(else_block.statements.is_empty());
        assert!(matches!(else_block.tail.kind, ExprKind::If { .. }));
    }

    #[test]
    fn ok_and_err_parse_as_expressions() {
        let c = parse_str("commons x { fn f(n: Int) -> Result[Int, String] { Ok(n) } }").unwrap();
        let CommonsItem::Fn(f) = &c.items[0] else {
            panic!()
        };
        assert!(matches!(f.body.tail.kind, ExprKind::Ok(_)));

        let c =
            parse_str("commons x { fn f(n: Int) -> Result[Int, String] { Err(\"x\") } }").unwrap();
        let CommonsItem::Fn(f) = &c.items[0] else {
            panic!()
        };
        assert!(matches!(f.body.tail.kind, ExprKind::Err(_)));
    }

    #[test]
    fn question_postfix_parses() {
        let c = parse_str(
            "commons x { type T = Int where Positive\n fn f(n: Int) -> Result[T, ValidationError] { let x = T.of(n)?\n Ok(x) } }",
        )
        .unwrap();
        let CommonsItem::Fn(f) = &c.items[1] else {
            panic!()
        };
        let Statement::Let(l) = &f.body.statements[0] else {
            panic!("expected a pure `let` statement");
        };
        assert!(matches!(l.value.kind, ExprKind::Question(_)));
    }

    #[test]
    fn constructor_call_parses() {
        let c = parse_str(
            "commons x { type T = Int where Positive\n fn f(n: Int) -> Result[T, ValidationError] { T.of(n) } }",
        )
        .unwrap();
        let CommonsItem::Fn(f) = &c.items[1] else {
            panic!()
        };
        // v0.2: T.of(n) parses as a MethodCall with receiver Ident("T"); the
        // checker reinterprets it as a static call by noticing T is a type.
        let ExprKind::MethodCall {
            receiver, method, ..
        } = &f.body.tail.kind
        else {
            panic!("expected MethodCall, got {:?}", f.body.tail.kind)
        };
        let ExprKind::Ident(id) = &receiver.kind else {
            panic!("expected receiver Ident");
        };
        assert_eq!(id.name, "T");
        assert_eq!(method.name, "of");
    }

    #[test]
    fn result_type_ref_parses() {
        let c = parse_str("commons x { fn f(n: Int) -> Result[Int, String] { Ok(n) } }").unwrap();
        let CommonsItem::Fn(f) = &c.items[0] else {
            panic!()
        };
        assert!(matches!(f.return_type, TypeRef::Result(_, _, _)));
    }

    #[test]
    fn result_missing_arg_count_errors() {
        let errs = parse_str("commons x { fn f(n: Int) -> Result[Int] { Ok(n) } }").unwrap_err();
        assert_eq!(errs[0].category, "bynk.parse.generic_arg_count");
    }

    #[test]
    fn field_access_parses_in_v0_2() {
        // v0.2: field access is supported (the type checker validates the
        // field exists on the receiver's type). Parser-level acceptance:
        let c =
            parse_str("commons x { type R = { foo: Int }\n fn f(r: R) -> Int { r.foo } }").unwrap();
        let CommonsItem::Fn(f) = &c.items[1] else {
            panic!()
        };
        assert!(matches!(f.body.tail.kind, ExprKind::FieldAccess { .. }));
    }

    // -- v1.1 trivia attachment --

    #[test]
    fn leading_line_comment_attaches_to_next_decl() {
        let src = "commons x {\n-- explain the type\ntype T = Int where NonNegative\n}";
        let c = parse_str(src).unwrap();
        let CommonsItem::Type(t) = &c.items[0] else {
            panic!()
        };
        assert_eq!(t.trivia.leading, vec![" explain the type".to_string()]);
        assert!(t.trivia.trailing.is_none());
    }

    #[test]
    fn trailing_line_comment_attaches_to_prev_decl() {
        let src = "commons x {\ntype T = Int where NonNegative  -- trailing note\n}";
        let c = parse_str(src).unwrap();
        let CommonsItem::Type(t) = &c.items[0] else {
            panic!()
        };
        assert!(t.trivia.leading.is_empty());
        assert_eq!(t.trivia.trailing.as_deref(), Some(" trailing note"));
    }

    #[test]
    fn grouped_leading_comments_attach_together() {
        let src = "commons x {\n-- one\n-- two\n-- three\ntype T = Int where Positive\n}";
        let c = parse_str(src).unwrap();
        let CommonsItem::Type(t) = &c.items[0] else {
            panic!()
        };
        assert_eq!(
            t.trivia.leading,
            vec![" one".to_string(), " two".to_string(), " three".to_string()],
        );
    }

    #[test]
    fn comment_with_doc_block_keeps_both() {
        // Both `-- intro` and the doc block should attach to the type decl.
        let src = "commons x {\n-- intro\n---\ndocs\n---\ntype T = Int where Positive\n}";
        let c = parse_str(src).unwrap();
        let CommonsItem::Type(t) = &c.items[0] else {
            panic!()
        };
        assert_eq!(t.trivia.leading, vec![" intro".to_string()]);
        assert_eq!(t.documentation.as_deref(), Some("docs"));
    }

    #[test]
    fn comment_before_let_statement_attaches() {
        let src = "commons x {\nfn f(n: Int) -> Int {\n-- pick a value\nlet y = n + 1\ny\n}\n}";
        let c = parse_str(src).unwrap();
        let CommonsItem::Fn(f) = &c.items[0] else {
            panic!()
        };
        let Statement::Let(l) = &f.body.statements[0] else {
            panic!()
        };
        assert_eq!(l.trivia.leading, vec![" pick a value".to_string()]);
    }

    #[test]
    fn comment_before_tail_attaches_to_block_tail() {
        let src = "commons x {\nfn f(n: Int) -> Int {\nlet y = n + 1\n-- result\ny\n}\n}";
        let c = parse_str(src).unwrap();
        let CommonsItem::Fn(f) = &c.items[0] else {
            panic!()
        };
        assert_eq!(f.body.tail_leading_comments, vec![" result".to_string()],);
    }

    /// Drift guard: every alphabetic keyword the lexer declares must be
    /// classified by `is_reserved_keyword`, or be one of the *contextual*
    /// keywords `expect_ident` deliberately admits as identifiers
    /// (`on`/`suite`/`case`). Everything else in this codebase that can
    /// drift has a guard; this predicate had silently fallen 17 keywords
    /// behind, degrading the reserved-keyword diagnostic to the generic
    /// expected-token one.
    #[test]
    fn is_reserved_keyword_covers_every_lexer_keyword() {
        let lexer_src = include_str!("lexer.rs");
        let mut words = Vec::new();
        for line in lexer_src.lines() {
            let t = line.trim();
            if let Some(rest) = t.strip_prefix("#[token(\"")
                && let Some(word) = rest.split('"').next()
                && word.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                && word.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                words.push(word.to_string());
            }
        }
        assert!(
            words.len() > 30,
            "keyword extraction looks broken: only {} words",
            words.len()
        );
        // Contextual keywords double as identifiers (see `expect_ident`).
        const CONTEXTUAL: &[&str] = &["on", "suite", "case"];
        let mut unclassified = Vec::new();
        for word in &words {
            let tokens = crate::lexer::tokenize(word).expect("keyword lexes");
            let kind = tokens.first().expect("keyword yields a token").kind;
            if !is_reserved_keyword(kind) && !CONTEXTUAL.contains(&word.as_str()) {
                unclassified.push(word.clone());
            }
        }
        assert!(
            unclassified.is_empty(),
            "keywords missing from is_reserved_keyword (add them, or document \
             them as contextual): {unclassified:?}"
        );
    }

    /// Fuzz-found (#516): a context-only keyword at item position in a
    /// commons errors without consuming the token, and the recovery sync
    /// stops at exactly that keyword — without a progress guard the item
    /// loop re-reported the same error until memory ran out.
    #[test]
    fn recovery_makes_progress_on_context_only_keyword_in_commons() {
        let src = "commons demo\n\ncapability Logger {\n  fn log(m: String) -> Effect[()]\n}\n";
        let tokens = crate::lexer::tokenize(src).unwrap();
        let (unit, errors) = parse_unit_with_recovery(&tokens, src);
        assert!(unit.is_some(), "the commons header still parses");
        assert!(
            errors
                .iter()
                .any(|e| e.category == "bynk.capability.outside_context"),
            "the misplaced capability is reported: {errors:?}"
        );
        // Termination is the real assertion (this used to OOM); a bounded,
        // non-repeating error list is the observable proxy.
        assert!(errors.len() < 10, "recovery repeated itself: {errors:?}");
    }

    #[test]
    fn trailing_file_comment_becomes_unit_trailing() {
        // A comment after the last item but before EOF (fragment form)
        // becomes the commons body's trailing comments so the formatter
        // can preserve it.
        let src = "commons x\n\ntype T = Int where Positive\n-- afterword\n";
        let c = parse_str(src).unwrap();
        assert_eq!(c.trailing_comments, vec![" afterword".to_string()]);
    }

    // ---- #636: `if`/`match` condition vs record construction ----

    /// Parse `body` as the tail expression of a fn and return its kind.
    fn body_tail(body: &str) -> ExprKind {
        let src = format!("commons x\n\nfn f() -> Int {{\n  {body}\n}}\n");
        let c = parse_str(&src).unwrap_or_else(|e| panic!("parse failed for {body:?}: {e:?}"));
        let CommonsItem::Fn(f) = &c.items[0] else {
            panic!("expected fn, got {:?}", c.items[0]);
        };
        f.body.tail.kind.clone()
    }

    fn body_err(body: &str) -> Vec<CompileError> {
        let src = format!("commons x\n\nfn f() -> Int {{\n  {body}\n}}\n");
        parse_str(&src).expect_err(&format!("expected a parse error for {body:?}"))
    }

    #[test]
    fn if_condition_ending_in_ident_does_not_swallow_a_single_ident_branch() {
        // #636: `ready { result }` shares its shape with a shorthand-field
        // record construction. In condition position the branch must win.
        for src in [
            "if ready { result } else { fallback }",
            "if ready { fallback } else { result }",
            "if !ready { result } else { fallback }",
            "if a == b { result } else { fallback }",
            "if a && b { result } else { fallback }",
        ] {
            let ExprKind::If {
                then_block,
                else_block,
                ..
            } = body_tail(src)
            else {
                panic!("expected If for {src:?}, got {:?}", body_tail(src));
            };
            // Both branches carry a bare-identifier tail — proof the `{ … }`
            // was read as a block, not consumed as a record by the condition.
            assert!(
                matches!(&then_block.tail.kind, ExprKind::Ident(_)),
                "then-branch tail not an ident for {src:?}: {:?}",
                then_block.tail.kind,
            );
            assert!(
                matches!(&else_block.tail.kind, ExprKind::Ident(_)),
                "else-branch tail not an ident for {src:?}: {:?}",
                else_block.tail.kind,
            );
        }
    }

    #[test]
    fn else_less_if_with_single_ident_branch_parses() {
        // The no-`else` reproduction: previously errored `found `}``.
        let ExprKind::If { then_block, .. } = body_tail("if ready { result }") else {
            panic!("expected If");
        };
        assert!(matches!(&then_block.tail.kind, ExprKind::Ident(_)));
    }

    #[test]
    fn record_construction_still_parses_in_value_position() {
        // The restriction is confined to condition spines — an ordinary value
        // position still constructs records, including the shorthand tail form.
        assert!(matches!(
            body_tail("Point { x }"),
            ExprKind::RecordConstruction { .. }
        ));
        assert!(matches!(
            body_tail("Point { x: 1, y: 2 }"),
            ExprKind::RecordConstruction { .. }
        ));
        assert!(matches!(
            body_tail("Empty {}"),
            ExprKind::RecordConstruction { .. }
        ));
    }

    #[test]
    fn parenthesised_record_is_allowed_in_condition_head() {
        // A delimiter lifts the restriction: `(ready { result })` constructs a
        // record even in condition position (mirrors Rust's paren escape).
        let ExprKind::If { cond, .. } =
            body_tail("if (ready { result }) { branch } else { other }")
        else {
            panic!("expected If");
        };
        let ExprKind::Paren(inner) = &cond.kind else {
            panic!("expected a parenthesised condition, got {:?}", cond.kind);
        };
        assert!(
            matches!(&inner.kind, ExprKind::RecordConstruction { .. }),
            "parenthesised record in condition head should still construct: {:?}",
            inner.kind,
        );
    }

    #[test]
    fn record_in_call_arg_within_condition_still_constructs() {
        // The restriction is lifted through a call-argument delimiter, so a
        // record literal passed to a predicate in the condition still parses.
        let ExprKind::If { cond, .. } = body_tail("if check(Point { x: 1 }) { a } else { b }")
        else {
            panic!("expected If");
        };
        let ExprKind::Call { args, .. } = &cond.kind else {
            panic!("expected Call in condition, got {:?}", cond.kind);
        };
        assert!(matches!(&args[0].kind, ExprKind::RecordConstruction { .. }));
    }

    #[test]
    fn safe_condition_shapes_are_unaffected() {
        // Cases the issue lists as already-safe must stay safe.
        assert!(matches!(
            body_tail("if ready == true { result } else { fallback }"),
            ExprKind::If { .. }
        ));
        assert!(matches!(
            body_tail("if (ready) { result } else { fallback }"),
            ExprKind::If { .. }
        ));
        assert!(matches!(
            body_tail("if ready { \"a\" } else { \"b\" }"),
            ExprKind::If { .. }
        ));
    }

    #[test]
    fn empty_match_reports_its_own_diagnostic() {
        // #636: `match result {}` once parsed `result {}` as an empty record,
        // masking `bynk.parse.empty_match`. The intended diagnostic is now
        // reachable.
        let errs = body_err("match result {}");
        assert!(
            errs.iter().any(|e| e.category == "bynk.parse.empty_match"),
            "expected empty_match; got {errs:?}",
        );
    }

    #[test]
    fn match_discriminant_ending_in_ident_parses() {
        // A `match` over a bare-identifier discriminant reaches its arm list.
        assert!(matches!(
            body_tail("match ready { x => x }"),
            ExprKind::Match { .. }
        ));
    }

    #[test]
    fn unparenthesised_record_in_condition_head_now_errors() {
        // #636 narrowing (matches Rust): a record literal in condition *head*
        // position must be parenthesised. Unparenthesised, `Point` reads as the
        // discriminant and `{ x: 1 }` as the arm list, whose first "arm" `x: 1`
        // is not an arm — so the parse fails. Pinned so the divergence from the
        // (still-accepting) tree-sitter grammar is deliberate, not a bug.
        assert!(
            !body_err("match Point { x: 1 } { p => p }").is_empty(),
            "unparenthesised record discriminant should not parse",
        );
        // Parenthesised, the record is the discriminant and the match parses.
        let ExprKind::Match { discriminant, .. } = body_tail("match (Point { x: 1 }) { p => p }")
        else {
            panic!("expected Match for the parenthesised form");
        };
        let ExprKind::Paren(inner) = &discriminant.kind else {
            panic!(
                "expected a parenthesised discriminant, got {:?}",
                discriminant.kind
            );
        };
        assert!(matches!(&inner.kind, ExprKind::RecordConstruction { .. }));
    }
}
