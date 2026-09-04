//! Parser that produces Syntax nodes instead of Value
//!
//! This parser is symbol-table-free and preserves source spans on every node.
//! It does NOT:
//! - Intern symbols (leaves them as strings)
//! - Desugar quote forms to lists
//!
//! This is a parallel implementation to the existing Value-producing parser.

use super::token::{OwnedToken, SourceLoc};
use crate::syntax::{Span, Syntax, SyntaxArena, SyntaxKind};

/// A lexed token together with everything the syntax parser needs in order to
/// span it: its source location, its source byte length, and its start byte
/// offset.
///
/// This collapses what used to be four parallel `Vec`s — `tokens`, `locations`,
/// `lengths`, `byte_offsets`, all indexed by a single `pos` with nothing
/// keeping their lengths in agreement — into one `Vec<LexedToken>`. That makes
/// the agreement structural: each token carries its own three companions, so
/// they can no longer fall out of sync, be indexed past one another, or be
/// supplied in mismatched counts to the reader's internals.
struct LexedToken {
    token: OwnedToken,
    loc: SourceLoc,
    len: usize,
    byte_offset: usize,
}

/// Span width assumed for a token position with no recorded length — only
/// reachable past the end of the stream. One source character.
const DEFAULT_TOKEN_LEN: usize = 1;

pub struct SyntaxReader {
    tokens: Vec<LexedToken>,
    pos: usize,
    /// Where the nodes this reader builds are born. A field rather than a
    /// per-call argument: one source parses into one arena
    /// (docs/impl/syntax.md § "Where a node lives").
    arena: SyntaxArena,
}

impl SyntaxReader {
    /// Zip the lexer's parallel output columns into one `Vec<LexedToken>`. This
    /// is the single point where the four columns meet — and the only place a
    /// length mismatch between them could matter. A position absent from
    /// `locations`/`lengths`/`byte_offsets` (only possible if a caller passes
    /// ragged columns) falls back to the same defaults the old per-field
    /// accessors used.
    fn from_columns(
        tokens: Vec<OwnedToken>,
        locations: Vec<SourceLoc>,
        lengths: Vec<usize>,
        byte_offsets: Vec<usize>,
        arena: SyntaxArena,
    ) -> Self {
        let tokens = tokens
            .into_iter()
            .enumerate()
            .map(|(i, token)| LexedToken {
                token,
                loc: locations.get(i).cloned().unwrap_or_else(SourceLoc::start),
                len: lengths.get(i).copied().unwrap_or(DEFAULT_TOKEN_LEN),
                byte_offset: byte_offsets.get(i).copied().unwrap_or(0),
            })
            .collect();
        SyntaxReader {
            tokens,
            pos: 0,
            arena,
        }
    }

    pub fn new(
        tokens: Vec<OwnedToken>,
        locations: Vec<SourceLoc>,
        lengths: Vec<usize>,
        arena: SyntaxArena,
    ) -> Self {
        // No byte offsets supplied: every token defaults to offset 0, as before.
        Self::from_columns(tokens, locations, lengths, Vec::new(), arena)
    }

    pub fn with_byte_offsets(
        tokens: Vec<OwnedToken>,
        locations: Vec<SourceLoc>,
        lengths: Vec<usize>,
        byte_offsets: Vec<usize>,
        arena: SyntaxArena,
    ) -> Self {
        Self::from_columns(tokens, locations, lengths, byte_offsets, arena)
    }

    fn current(&self) -> Option<&OwnedToken> {
        self.tokens.get(self.pos).map(|t| &t.token)
    }

    fn current_location(&self) -> SourceLoc {
        // At or past the end, fall back to the last token's location (or the
        // start sentinel for an empty stream), as before.
        self.tokens
            .get(self.pos)
            .or_else(|| self.tokens.last())
            .map(|t| t.loc.clone())
            .unwrap_or_else(SourceLoc::start)
    }

    fn current_length(&self) -> usize {
        self.tokens
            .get(self.pos)
            .map(|t| t.len)
            .unwrap_or(DEFAULT_TOKEN_LEN)
    }

    fn current_byte_offset(&self) -> usize {
        self.tokens
            .get(self.pos)
            .map(|t| t.byte_offset)
            .unwrap_or(0)
    }

    /// Check if there are remaining tokens. Returns Some with error message if so.
    pub fn check_exhausted(&self) -> Option<String> {
        self.current().map(|token| {
            let loc = self.current_location();
            format!(
                "{}: unexpected token after expression: {:?}",
                loc.position(),
                token
            )
        })
    }

    fn advance(&mut self) -> Option<OwnedToken> {
        let token = self.tokens.get(self.pos).map(|t| t.token.clone());
        self.pos += 1;
        token
    }

    /// Build a span from byte offsets and source location.
    fn make_span(&self, byte_start: usize, byte_end: usize, loc: &SourceLoc) -> Span {
        let mut span = Span::new(byte_start, byte_end, loc.line as u32, loc.col as u32);
        if !loc.is_unknown() {
            span = span.with_file(loc.file.clone());
        }
        span
    }

    /// Try to read a single syntax form. Returns None at EOF.
    pub fn try_read(&mut self) -> Option<Result<Syntax, String>> {
        // Skip any leading comment tokens
        while matches!(self.current(), Some(OwnedToken::Comment(_))) {
            self.advance();
        }
        let token = self.current().cloned()?;
        let loc = self.current_location();
        let boff = self.current_byte_offset();
        Some(self.read_one(&token, &loc, boff))
    }

    /// Read a single syntax form. Returns error at EOF.
    pub fn read(&mut self) -> Result<Syntax, String> {
        match self.try_read() {
            Some(result) => result,
            None => {
                let loc = self.current_location();
                Err(format!("{}: unexpected end of input", loc.position()))
            }
        }
    }

    /// Read all remaining forms
    pub fn read_all(&mut self) -> Result<Vec<Syntax>, String> {
        let mut results = Vec::new();
        while let Some(result) = self.try_read() {
            results.push(result?);
        }
        Ok(results)
    }

    fn read_one(
        &mut self,
        token: &OwnedToken,
        loc: &SourceLoc,
        boff: usize,
    ) -> Result<Syntax, String> {
        match token {
            // Skip comment tokens inside compound forms
            OwnedToken::Comment(_) => {
                self.advance();
                self.read()
            }
            OwnedToken::LeftParen => self.read_list(loc, boff),
            OwnedToken::LeftBracket => self.read_array(loc, boff),
            OwnedToken::LeftBrace => self.read_struct(loc, boff),
            OwnedToken::ListSugar => self.read_list_sugar(loc, boff),
            OwnedToken::Pipe => self.read_set(loc, boff),
            OwnedToken::AtPipe => self.read_set_mut(loc, boff),
            OwnedToken::BytesBracket => self.read_bytes(loc, boff),
            OwnedToken::AtBytesBracket => self.read_bytes_mut(loc, boff),

            OwnedToken::Quote => {
                self.advance();
                let inner = self.read()?;
                let span = self.make_span(boff, inner.span.end as usize, loc);
                Ok(Syntax::new(SyntaxKind::Quote(self.arena.node(inner)), span))
            }
            OwnedToken::Quasiquote => {
                self.advance();
                let inner = self.read()?;
                let span = self.make_span(boff, inner.span.end as usize, loc);
                Ok(Syntax::new(
                    SyntaxKind::Quasiquote(self.arena.node(inner)),
                    span,
                ))
            }
            OwnedToken::Unquote => {
                self.advance();
                let inner = self.read()?;
                let span = self.make_span(boff, inner.span.end as usize, loc);
                Ok(Syntax::new(
                    SyntaxKind::Unquote(self.arena.node(inner)),
                    span,
                ))
            }
            OwnedToken::UnquoteSplicing => {
                self.advance();
                let inner = self.read()?;
                let span = self.make_span(boff, inner.span.end as usize, loc);
                Ok(Syntax::new(
                    SyntaxKind::UnquoteSplicing(self.arena.node(inner)),
                    span,
                ))
            }
            OwnedToken::Splice => {
                self.advance();
                let inner = self.read()?;
                let span = self.make_span(boff, inner.span.end as usize, loc);
                Ok(Syntax::new(
                    SyntaxKind::Splice(self.arena.node(inner)),
                    span,
                ))
            }

            OwnedToken::Integer(n) => {
                let span = self.make_span(boff, boff + self.current_length(), loc);
                self.advance();
                Ok(Syntax::new(SyntaxKind::Int(*n), span))
            }
            OwnedToken::Float(f) => {
                let span = self.make_span(boff, boff + self.current_length(), loc);
                self.advance();
                Ok(Syntax::new(SyntaxKind::Float(*f), span))
            }
            OwnedToken::String(s) => {
                let span = self.make_span(boff, boff + self.current_length(), loc);
                self.advance();
                Ok(Syntax::new(SyntaxKind::String(self.arena.text(s)), span))
            }
            OwnedToken::Bool(b) => {
                let span = self.make_span(boff, boff + self.current_length(), loc);
                self.advance();
                Ok(Syntax::new(SyntaxKind::Bool(*b), span))
            }
            OwnedToken::Nil => {
                let span = self.make_span(boff, boff + self.current_length(), loc);
                self.advance();
                Ok(Syntax::new(SyntaxKind::Nil, span))
            }
            OwnedToken::Symbol(s) => {
                let span = self.make_span(boff, boff + self.current_length(), loc);
                self.advance();
                Ok(Syntax::new(SyntaxKind::Symbol(self.arena.text(s)), span))
            }
            OwnedToken::Keyword(s) => {
                let span = self.make_span(boff, boff + self.current_length(), loc);
                self.advance();
                Ok(Syntax::new(SyntaxKind::Keyword(self.arena.text(s)), span))
            }

            OwnedToken::RightParen => Err(format!(
                "{}: unexpected closing parenthesis",
                loc.position()
            )),
            OwnedToken::RightBracket => {
                Err(format!("{}: unexpected closing bracket", loc.position()))
            }
            OwnedToken::RightBrace => Err(format!("{}: unexpected closing brace", loc.position())),
        }
    }

    fn read_set(&mut self, start_loc: &SourceLoc, start_boff: usize) -> Result<Syntax, String> {
        self.advance(); // skip opening |
        let mut elements = Vec::new();

        loop {
            match self.current() {
                None => {
                    return Err(format!(
                        "{}: unterminated set literal (missing closing |)",
                        start_loc.position()
                    ));
                }
                Some(OwnedToken::Pipe) => {
                    let end = self.current_byte_offset() + self.current_length();
                    self.advance();
                    let span = self.make_span(start_boff, end, start_loc);
                    return Ok(Syntax::new(
                        SyntaxKind::Set(self.arena.nodes(&elements)),
                        span,
                    ));
                }
                Some(OwnedToken::Comment(_)) => {
                    self.advance();
                    continue;
                }
                _ => elements.push(self.read()?),
            }
        }
    }

    fn read_set_mut(&mut self, start_loc: &SourceLoc, start_boff: usize) -> Result<Syntax, String> {
        self.advance(); // skip opening @|
        let mut elements = Vec::new();

        loop {
            match self.current() {
                None => {
                    return Err(format!(
                        "{}: unterminated mutable set literal (missing closing |)",
                        start_loc.position()
                    ));
                }
                Some(OwnedToken::Pipe) => {
                    let end = self.current_byte_offset() + self.current_length();
                    self.advance();
                    let span = self.make_span(start_boff, end, start_loc);
                    return Ok(Syntax::new(
                        SyntaxKind::SetMut(self.arena.nodes(&elements)),
                        span,
                    ));
                }
                Some(OwnedToken::Comment(_)) => {
                    self.advance();
                    continue;
                }
                _ => elements.push(self.read()?),
            }
        }
    }

    fn read_bytes(&mut self, start_loc: &SourceLoc, start_boff: usize) -> Result<Syntax, String> {
        self.advance(); // skip b[
        let mut elements = Vec::new();
        loop {
            match self.current() {
                None => {
                    return Err(format!(
                        "{}: unterminated bytes literal (missing closing ])",
                        start_loc.position()
                    ));
                }
                Some(OwnedToken::RightBracket) => {
                    let end = self.current_byte_offset() + self.current_length();
                    self.advance();
                    let span = self.make_span(start_boff, end, start_loc);
                    return Ok(Syntax::new(
                        SyntaxKind::Bytes(self.arena.nodes(&elements)),
                        span,
                    ));
                }
                Some(OwnedToken::Comment(_)) => {
                    self.advance();
                    continue;
                }
                _ => elements.push(self.read()?),
            }
        }
    }

    fn read_bytes_mut(
        &mut self,
        start_loc: &SourceLoc,
        start_boff: usize,
    ) -> Result<Syntax, String> {
        self.advance(); // skip @b[
        let mut elements = Vec::new();
        loop {
            match self.current() {
                None => {
                    return Err(format!(
                        "{}: unterminated @bytes literal (missing closing ])",
                        start_loc.position()
                    ));
                }
                Some(OwnedToken::RightBracket) => {
                    let end = self.current_byte_offset() + self.current_length();
                    self.advance();
                    let span = self.make_span(start_boff, end, start_loc);
                    return Ok(Syntax::new(
                        SyntaxKind::BytesMut(self.arena.nodes(&elements)),
                        span,
                    ));
                }
                Some(OwnedToken::Comment(_)) => {
                    self.advance();
                    continue;
                }
                _ => elements.push(self.read()?),
            }
        }
    }

    fn read_list(&mut self, start_loc: &SourceLoc, start_boff: usize) -> Result<Syntax, String> {
        self.advance(); // skip (
        let mut elements: Vec<Syntax> = Vec::new();
        // Track the innermost unclosed opening delimiter for better error messages
        let mut innermost_unclosed: Option<SourceLoc> = None;

        loop {
            match self.current() {
                None => {
                    // Point at innermost unclosed paren if we have one, else the outermost
                    let point_at = innermost_unclosed.as_ref().unwrap_or(start_loc);
                    let depth = 1 + elements
                        .iter()
                        .filter(|e| matches!(&e.kind, SyntaxKind::List(_)))
                        .count()
                        .min(1); // approximate depth
                    return Err(format!(
                        "{}: unterminated list ({} closing paren{} needed)",
                        point_at.position(),
                        depth,
                        if depth > 1 { "s" } else { "" }
                    ));
                }
                Some(OwnedToken::RightParen) => {
                    let end = self.current_byte_offset() + self.current_length();
                    self.advance();
                    let span = self.make_span(start_boff, end, start_loc);
                    return Ok(Syntax::new(
                        SyntaxKind::List(self.arena.nodes(&elements)),
                        span,
                    ));
                }
                Some(OwnedToken::Comment(_)) => {
                    self.advance();
                    continue;
                }
                Some(OwnedToken::Pipe) => {
                    let set_loc = self.current_location();
                    let set_boff = self.current_byte_offset();
                    elements.push(self.read_set(&set_loc, set_boff)?);
                    continue;
                }
                Some(OwnedToken::LeftParen) => {
                    // Track this as potentially the innermost unclosed paren
                    innermost_unclosed = Some(self.current_location());
                    elements.push(self.read()?);
                }
                _ => elements.push(self.read()?),
            }
        }
    }

    fn read_array(&mut self, start_loc: &SourceLoc, start_boff: usize) -> Result<Syntax, String> {
        self.advance(); // skip [
        let mut elements = Vec::new();

        loop {
            match self.current() {
                None => {
                    return Err(format!(
                        "{}: unterminated tuple (missing closing bracket)",
                        start_loc.position()
                    ));
                }
                Some(OwnedToken::RightBracket) => {
                    let end = self.current_byte_offset() + self.current_length();
                    self.advance();
                    let span = self.make_span(start_boff, end, start_loc);
                    return Ok(Syntax::new(
                        SyntaxKind::Array(self.arena.nodes(&elements)),
                        span,
                    ));
                }
                Some(OwnedToken::Comment(_)) => {
                    self.advance();
                    continue;
                }
                Some(OwnedToken::Pipe) => {
                    let set_loc = self.current_location();
                    let set_boff = self.current_byte_offset();
                    elements.push(self.read_set(&set_loc, set_boff)?);
                    continue;
                }
                _ => elements.push(self.read()?),
            }
        }
    }

    fn read_struct(&mut self, start_loc: &SourceLoc, start_boff: usize) -> Result<Syntax, String> {
        self.advance(); // skip {
        let mut elements = Vec::new();

        loop {
            match self.current() {
                None => {
                    return Err(format!(
                        "{}: unterminated struct (missing closing brace)",
                        start_loc.position()
                    ));
                }
                Some(OwnedToken::RightBrace) => {
                    let end = self.current_byte_offset() + self.current_length();
                    self.advance();
                    let span = self.make_span(start_boff, end, start_loc);
                    return Ok(Syntax::new(
                        SyntaxKind::Struct(self.arena.nodes(&elements)),
                        span,
                    ));
                }
                Some(OwnedToken::Comment(_)) => {
                    self.advance();
                    continue;
                }
                Some(OwnedToken::Pipe) => {
                    let set_loc = self.current_location();
                    let set_boff = self.current_byte_offset();
                    elements.push(self.read_set(&set_loc, set_boff)?);
                    continue;
                }
                _ => elements.push(self.read()?),
            }
        }
    }

    fn read_list_sugar(
        &mut self,
        start_loc: &SourceLoc,
        start_boff: usize,
    ) -> Result<Syntax, String> {
        self.advance(); // skip @

        match self.current() {
            Some(OwnedToken::LeftBracket) => {
                // @[...] produces an array literal
                self.advance(); // skip [
                let mut elements = Vec::new();

                loop {
                    match self.current() {
                        None => {
                            return Err(format!(
                                "{}: unterminated array literal",
                                start_loc.position()
                            ));
                        }
                        Some(OwnedToken::RightBracket) => {
                            let end = self.current_byte_offset() + self.current_length();
                            self.advance();
                            let span = self.make_span(start_boff, end, start_loc);
                            return Ok(Syntax::new(
                                SyntaxKind::ArrayMut(self.arena.nodes(&elements)),
                                span,
                            ));
                        }
                        Some(OwnedToken::Comment(_)) => {
                            self.advance();
                            continue;
                        }
                        _ => elements.push(self.read()?),
                    }
                }
            }
            Some(OwnedToken::LeftBrace) => {
                // @{...} produces a table literal
                self.advance(); // skip {
                let mut elements = Vec::new();

                loop {
                    match self.current() {
                        None => {
                            return Err(format!(
                                "{}: unterminated table literal",
                                start_loc.position()
                            ));
                        }
                        Some(OwnedToken::RightBrace) => {
                            let end = self.current_byte_offset() + self.current_length();
                            self.advance();
                            let span = self.make_span(start_boff, end, start_loc);
                            return Ok(Syntax::new(
                                SyntaxKind::StructMut(self.arena.nodes(&elements)),
                                span,
                            ));
                        }
                        Some(OwnedToken::Comment(_)) => {
                            self.advance();
                            continue;
                        }
                        _ => elements.push(self.read()?),
                    }
                }
            }
            Some(OwnedToken::String(s)) => {
                // @"..." is a mutable string literal
                let string_val = self.arena.text(s);
                let end = self.current_byte_offset() + self.current_length();
                self.advance(); // skip the string token
                let span = self.make_span(start_boff, end, start_loc);
                Ok(Syntax::new(SyntaxKind::StringMut(string_val), span))
            }
            _ => Err(format!(
                "{}: @ must be followed by [...], {{...}}, |...|, or \"...\"",
                start_loc.position()
            )),
        }
    }
}

#[cfg(test)]
#[path = "syntax_tests/mod.rs"]
mod tests;
