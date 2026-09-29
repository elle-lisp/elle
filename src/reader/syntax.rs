// audited: 2026-09-28
//! Parser that produces Syntax nodes instead of Value
//!
//! This parser is symbol-table-free and preserves source spans on every node.
//! It does NOT:
//! - Intern symbols (leaves them as strings)
//! - Desugar quote forms to lists
//!
//! `Reader` in parser.rs parses the same tokens to `Value`.
//! docs/impl/reader.md

use super::token::{OwnedToken, SourceLoc};
use crate::syntax::{Span, Syntax, SyntaxArena, SyntaxKind};

mod collections;

/// A lexed token together with everything the syntax parser needs in order to
/// span it: its source location, its source byte length, and its start byte
/// offset.
///
/// Each token carries its own three companions, so they cannot fall out of
/// sync or be indexed past one another inside the reader.
struct LexedToken {
    token: OwnedToken,
    loc: SourceLoc,
    len: usize,
    byte_offset: usize,
}

/// Span width assumed for a token position with no recorded length — only
/// reachable past the end of the stream. One source character.
const DEFAULT_TOKEN_LEN: usize = 1;

#[derive(Clone, Copy, PartialEq, Eq)]
enum OpenFormKind {
    List,
    Array,
    Struct,
    Set,
    Bytes,
}

impl OpenFormKind {
    fn name(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Array => "array",
            Self::Struct => "struct",
            Self::Set => "set",
            Self::Bytes => "bytes literal",
        }
    }

    fn delimiter(self) -> &'static str {
        match self {
            Self::List => "paren",
            Self::Array | Self::Bytes => "bracket",
            Self::Struct => "brace",
            Self::Set => "|",
        }
    }

    fn delimiter_plural(self) -> &'static str {
        match self {
            Self::List => "parens",
            Self::Array | Self::Bytes => "brackets",
            Self::Struct => "braces",
            Self::Set => "pipes",
        }
    }
}

struct OpenForm {
    kind: OpenFormKind,
    loc: SourceLoc,
}

pub struct SyntaxReader {
    tokens: Vec<LexedToken>,
    pos: usize,
    open_forms: Vec<OpenForm>,
    /// Where the nodes this reader builds are born. A field rather than a
    /// per-call argument: one source parses into one arena
    /// (docs/impl/syntax.md).
    arena: SyntaxArena,
}

impl SyntaxReader {
    /// Zip the lexer's parallel output columns into one `Vec<LexedToken>`. This
    /// is the single point where the four columns meet — and the only place a
    /// length mismatch between them could matter. A position absent from
    /// `locations`/`lengths`/`byte_offsets` (only possible if a caller passes
    /// ragged columns) falls back to the start location, a one-character
    /// length, and offset 0.
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
            open_forms: Vec::new(),
            arena,
        }
    }

    pub fn new(
        tokens: Vec<OwnedToken>,
        locations: Vec<SourceLoc>,
        lengths: Vec<usize>,
        arena: SyntaxArena,
    ) -> Self {
        // No byte offsets supplied: every token defaults to offset 0.
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
        // start sentinel for an empty stream).
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

    fn unterminated_collection(&self) -> String {
        let first = self
            .open_forms
            .first()
            .expect("unterminated collection without an open form");
        let depth = self.open_forms.len();
        let same_delimiter = self
            .open_forms
            .iter()
            .all(|form| form.kind.delimiter() == first.kind.delimiter());

        let detail = if !same_delimiter {
            format!("{depth} closing delimiters needed")
        } else if first.kind == OpenFormKind::List {
            format!(
                "{depth} closing paren{} needed",
                if depth == 1 { "" } else { "s" }
            )
        } else if depth == 1 {
            format!("missing closing {}", first.kind.delimiter())
        } else {
            format!("{depth} closing {} needed", first.kind.delimiter_plural())
        };

        format!(
            "{}: unterminated {} ({detail})",
            first.loc.position(),
            first.kind.name()
        )
    }

    fn open_form(&mut self, kind: OpenFormKind, loc: &SourceLoc) {
        self.open_forms.push(OpenForm {
            kind,
            loc: loc.clone(),
        });
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
}

#[cfg(test)]
#[path = "syntax_tests/mod.rs"]
mod tests;
