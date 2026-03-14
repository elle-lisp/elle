//! Parser that produces Syntax nodes instead of Value
//!
//! This parser is symbol-table-free and preserves source spans on every node.
//! It does NOT:
//! - Intern symbols (leaves them as strings)
//! - Desugar quote forms to lists
//!
//! This is a parallel implementation to the existing Value-producing parser.

use super::token::{OwnedToken, SourceLoc};
use crate::syntax::{Span, Syntax, SyntaxKind};

pub struct SyntaxReader {
    tokens: Vec<OwnedToken>,
    locations: Vec<SourceLoc>,
    lengths: Vec<usize>,
    pos: usize,
}

impl SyntaxReader {
    pub fn new(tokens: Vec<OwnedToken>, locations: Vec<SourceLoc>, lengths: Vec<usize>) -> Self {
        SyntaxReader {
            tokens,
            locations,
            lengths,
            pos: 0,
        }
    }

    fn current(&self) -> Option<&OwnedToken> {
        self.tokens.get(self.pos)
    }

    fn current_location(&self) -> SourceLoc {
        self.locations.get(self.pos).cloned().unwrap_or_else(|| {
            // If we're past the end, use the last location
            self.locations
                .last()
                .cloned()
                .unwrap_or_else(SourceLoc::start)
        })
    }

    fn current_length(&self) -> usize {
        self.lengths.get(self.pos).copied().unwrap_or(1)
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
        let token = self.current().cloned();
        self.pos += 1;
        token
    }

    /// Convert a SourceLoc to a Span
    fn source_loc_to_span(&self, loc: &SourceLoc, end_offset: usize) -> Span {
        let file = if loc.is_unknown() {
            None
        } else {
            Some(loc.file.clone())
        };

        let mut span = Span::new(0, end_offset, loc.line as u32, loc.col as u32);
        if let Some(f) = file {
            span = span.with_file(f);
        }
        span
    }

    /// Try to read a single syntax form. Returns None at EOF.
    pub fn try_read(&mut self) -> Option<Result<Syntax, String>> {
        let token = self.current().cloned()?;
        let loc = self.current_location();
        Some(self.read_one(&token, &loc))
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
        while self.current().is_some() {
            results.push(self.read()?);
        }
        Ok(results)
    }

    fn read_one(&mut self, token: &OwnedToken, loc: &SourceLoc) -> Result<Syntax, String> {
        match token {
            OwnedToken::LeftParen => self.read_list(loc),
            OwnedToken::LeftBracket => self.read_array(loc),
            OwnedToken::LeftBrace => self.read_struct(loc),
            OwnedToken::ListSugar => self.read_list_sugar(loc),
            OwnedToken::Pipe => self.read_set(loc),
            OwnedToken::AtPipe => self.read_set_mut(loc),

            OwnedToken::Quote => {
                let len = self.current_length();
                self.advance();
                let inner = self.read()?;
                let start_span = self.source_loc_to_span(loc, loc.col + len);
                let span = start_span.merge(&inner.span);
                Ok(Syntax::new(SyntaxKind::Quote(Box::new(inner)), span))
            }
            OwnedToken::Quasiquote => {
                let len = self.current_length();
                self.advance();
                let inner = self.read()?;
                let start_span = self.source_loc_to_span(loc, loc.col + len);
                let span = start_span.merge(&inner.span);
                Ok(Syntax::new(SyntaxKind::Quasiquote(Box::new(inner)), span))
            }
            OwnedToken::Unquote => {
                let len = self.current_length();
                self.advance();
                let inner = self.read()?;
                let start_span = self.source_loc_to_span(loc, loc.col + len);
                let span = start_span.merge(&inner.span);
                Ok(Syntax::new(SyntaxKind::Unquote(Box::new(inner)), span))
            }
            OwnedToken::UnquoteSplicing => {
                let len = self.current_length();
                self.advance();
                let inner = self.read()?;
                let start_span = self.source_loc_to_span(loc, loc.col + len);
                let span = start_span.merge(&inner.span);
                Ok(Syntax::new(
                    SyntaxKind::UnquoteSplicing(Box::new(inner)),
                    span,
                ))
            }
            OwnedToken::Splice => {
                let len = self.current_length();
                self.advance();
                let inner = self.read()?;
                let start_span = self.source_loc_to_span(loc, loc.col + len);
                let span = start_span.merge(&inner.span);
                Ok(Syntax::new(SyntaxKind::Splice(Box::new(inner)), span))
            }

            OwnedToken::Integer(n) => {
                let span = self.source_loc_to_span(loc, loc.col + self.current_length());
                self.advance();
                Ok(Syntax::new(SyntaxKind::Int(*n), span))
            }
            OwnedToken::Float(f) => {
                let span = self.source_loc_to_span(loc, loc.col + self.current_length());
                self.advance();
                Ok(Syntax::new(SyntaxKind::Float(*f), span))
            }
            OwnedToken::String(s) => {
                let span = self.source_loc_to_span(loc, loc.col + self.current_length());
                self.advance();
                Ok(Syntax::new(SyntaxKind::String(s.clone()), span))
            }
            OwnedToken::Bool(b) => {
                let span = self.source_loc_to_span(loc, loc.col + self.current_length());
                self.advance();
                Ok(Syntax::new(SyntaxKind::Bool(*b), span))
            }
            OwnedToken::Nil => {
                let span = self.source_loc_to_span(loc, loc.col + self.current_length());
                self.advance();
                Ok(Syntax::new(SyntaxKind::Nil, span))
            }
            OwnedToken::Symbol(s) => {
                let span = self.source_loc_to_span(loc, loc.col + self.current_length());
                self.advance();
                Ok(Syntax::new(SyntaxKind::Symbol(s.clone()), span))
            }
            OwnedToken::Keyword(s) => {
                let span = self.source_loc_to_span(loc, loc.col + self.current_length());
                self.advance();
                Ok(Syntax::new(SyntaxKind::Keyword(s.clone()), span))
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

    fn read_set(&mut self, start_loc: &SourceLoc) -> Result<Syntax, String> {
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
                    let end_loc = self.current_location();
                    self.advance();
                    let span = self.merge_spans(start_loc, &end_loc, &elements);
                    return Ok(Syntax::new(SyntaxKind::Set(elements), span));
                }
                _ => elements.push(self.read()?),
            }
        }
    }

    fn read_set_mut(&mut self, start_loc: &SourceLoc) -> Result<Syntax, String> {
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
                    let end_loc = self.current_location();
                    self.advance();
                    let span = self.merge_spans(start_loc, &end_loc, &elements);
                    return Ok(Syntax::new(SyntaxKind::SetMut(elements), span));
                }
                _ => elements.push(self.read()?),
            }
        }
    }

    fn read_list(&mut self, start_loc: &SourceLoc) -> Result<Syntax, String> {
        self.advance(); // skip (
        let mut elements = Vec::new();

        loop {
            match self.current() {
                None => {
                    return Err(format!(
                        "{}: unterminated list (missing closing paren)",
                        start_loc.position()
                    ));
                }
                Some(OwnedToken::RightParen) => {
                    let end_loc = self.current_location();
                    self.advance();
                    let span = self.merge_spans(start_loc, &end_loc, &elements);
                    return Ok(Syntax::new(SyntaxKind::List(elements), span));
                }
                Some(OwnedToken::Pipe) => {
                    let set_loc = self.current_location();
                    elements.push(self.read_set(&set_loc)?);
                    continue;
                }
                _ => elements.push(self.read()?),
            }
        }
    }

    fn read_array(&mut self, start_loc: &SourceLoc) -> Result<Syntax, String> {
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
                    let end_loc = self.current_location();
                    self.advance();
                    let span = self.merge_spans(start_loc, &end_loc, &elements);
                    return Ok(Syntax::new(SyntaxKind::Array(elements), span));
                }
                Some(OwnedToken::Pipe) => {
                    let set_loc = self.current_location();
                    elements.push(self.read_set(&set_loc)?);
                    continue;
                }
                _ => elements.push(self.read()?),
            }
        }
    }

    fn read_struct(&mut self, start_loc: &SourceLoc) -> Result<Syntax, String> {
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
                    let end_loc = self.current_location();
                    self.advance();

                    let span = self.merge_spans(start_loc, &end_loc, &elements);
                    return Ok(Syntax::new(SyntaxKind::Struct(elements), span));
                }
                Some(OwnedToken::Pipe) => {
                    let set_loc = self.current_location();
                    elements.push(self.read_set(&set_loc)?);
                    continue;
                }
                _ => elements.push(self.read()?),
            }
        }
    }

    fn read_list_sugar(&mut self, start_loc: &SourceLoc) -> Result<Syntax, String> {
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
                            let end_loc = self.current_location();
                            self.advance();

                            let span = self.merge_spans(start_loc, &end_loc, &elements);
                            return Ok(Syntax::new(SyntaxKind::ArrayMut(elements), span));
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
                            let end_loc = self.current_location();
                            self.advance();

                            let span = self.merge_spans(start_loc, &end_loc, &elements);
                            return Ok(Syntax::new(SyntaxKind::StructMut(elements), span));
                        }
                        _ => elements.push(self.read()?),
                    }
                }
            }
            Some(OwnedToken::String(s)) => {
                // @"..." is sugar for (thaw "...")
                let string_val = s.clone();
                let len = self.current_length();
                self.advance(); // skip the string token
                let span = self.source_loc_to_span(start_loc, start_loc.col + len + 1);
                let sym = Syntax::new(SyntaxKind::Symbol("thaw".to_string()), span.clone());
                let str_lit = Syntax::new(SyntaxKind::String(string_val), span.clone());
                Ok(Syntax::new(SyntaxKind::List(vec![sym, str_lit]), span))
            }
            _ => Err(format!(
                "{}: @ must be followed by [...], {{...}}, |...|, or \"...\"",
                start_loc.position()
            )),
        }
    }

    /// Merge spans from start location to end location, or use element spans if available
    fn merge_spans(&self, start_loc: &SourceLoc, end_loc: &SourceLoc, elements: &[Syntax]) -> Span {
        if elements.is_empty() {
            // Empty container - use delimiter span
            self.source_loc_to_span(start_loc, end_loc.col + 1)
        } else {
            // Merge from first element to last element
            elements[0].span.merge(&elements[elements.len() - 1].span)
        }
    }
}

#[cfg(test)]
#[path = "syntax_tests.rs"]
mod tests;
