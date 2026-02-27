//! Parser that produces Syntax nodes instead of Value
//!
//! This parser is symbol-table-free and preserves source spans on every node.
//! It does NOT:
//! - Intern symbols (leaves them as strings)
//! - Expand qualified symbols to (qualified-ref ...)
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
                    return Ok(Syntax::new(SyntaxKind::Tuple(elements), span));
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
                            return Ok(Syntax::new(SyntaxKind::Array(elements), span));
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
                            return Ok(Syntax::new(SyntaxKind::Table(elements), span));
                        }
                        _ => elements.push(self.read()?),
                    }
                }
            }
            Some(OwnedToken::String(s)) => {
                // @"..." is sugar for (string->buffer "...")
                let string_val = s.clone();
                let len = self.current_length();
                self.advance(); // skip the string token
                let span = self.source_loc_to_span(start_loc, start_loc.col + len + 1);
                let sym = Syntax::new(
                    SyntaxKind::Symbol("string->buffer".to_string()),
                    span.clone(),
                );
                let str_lit = Syntax::new(SyntaxKind::String(string_val), span.clone());
                Ok(Syntax::new(SyntaxKind::List(vec![sym, str_lit]), span))
            }
            _ => Err(format!(
                "{}: @ must be followed by [...], {{...}}, or \"...\"",
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
mod tests {
    use super::*;
    use crate::reader::Lexer;

    fn lex_and_parse(input: &str) -> Result<Syntax, String> {
        let mut lexer = Lexer::new(input);
        let mut tokens = Vec::new();
        let mut locations = Vec::new();
        let mut lengths = Vec::new();

        while let Some(token_with_loc) = lexer.next_token_with_loc()? {
            tokens.push(OwnedToken::from(token_with_loc.token));
            locations.push(token_with_loc.loc);
            lengths.push(token_with_loc.len);
        }

        let mut reader = SyntaxReader::new(tokens, locations, lengths);
        reader.read()
    }

    fn lex_and_parse_all(input: &str) -> Result<Vec<Syntax>, String> {
        let mut lexer = Lexer::new(input);
        let mut tokens = Vec::new();
        let mut locations = Vec::new();
        let mut lengths = Vec::new();

        while let Some(token_with_loc) = lexer.next_token_with_loc()? {
            tokens.push(OwnedToken::from(token_with_loc.token));
            locations.push(token_with_loc.loc);
            lengths.push(token_with_loc.len);
        }

        let mut reader = SyntaxReader::new(tokens, locations, lengths);
        reader.read_all()
    }

    // Atoms
    #[test]
    fn test_parse_integer() {
        let result = lex_and_parse("42").unwrap();
        assert!(matches!(result.kind, SyntaxKind::Int(42)));
    }

    #[test]
    fn test_parse_float() {
        let result = lex_and_parse("2.71").unwrap();
        assert!(matches!(result.kind, SyntaxKind::Float(f) if (f - 2.71).abs() < 0.0001));
    }

    #[test]
    fn test_parse_string() {
        let result = lex_and_parse("\"hello\"").unwrap();
        assert!(matches!(result.kind, SyntaxKind::String(ref s) if s == "hello"));
    }

    #[test]
    fn test_parse_bool_true() {
        let result = lex_and_parse("#t").unwrap();
        assert!(matches!(result.kind, SyntaxKind::Bool(true)));
    }

    #[test]
    fn test_parse_bool_false() {
        let result = lex_and_parse("#f").unwrap();
        assert!(matches!(result.kind, SyntaxKind::Bool(false)));
    }

    #[test]
    fn test_parse_bool_true_word() {
        let result = lex_and_parse("true").unwrap();
        assert!(matches!(result.kind, SyntaxKind::Bool(true)));
    }

    #[test]
    fn test_parse_bool_false_word() {
        let result = lex_and_parse("false").unwrap();
        assert!(matches!(result.kind, SyntaxKind::Bool(false)));
    }

    #[test]
    fn test_parse_nil() {
        let result = lex_and_parse("nil").unwrap();
        assert!(matches!(result.kind, SyntaxKind::Nil));
    }

    #[test]
    fn test_parse_symbol() {
        let result = lex_and_parse("foo").unwrap();
        assert!(matches!(result.kind, SyntaxKind::Symbol(ref s) if s == "foo"));
    }

    #[test]
    fn test_parse_qualified_symbol() {
        // The lexer now handles module:name as a single qualified symbol
        let result = lex_and_parse("string:upcase").unwrap();
        assert!(matches!(result.kind, SyntaxKind::Symbol(ref s) if s == "string:upcase"));

        let result = lex_and_parse("math:abs").unwrap();
        assert!(matches!(result.kind, SyntaxKind::Symbol(ref s) if s == "math:abs"));

        // Keywords still work (colon at start)
        let result = lex_and_parse(":keyword").unwrap();
        assert!(matches!(result.kind, SyntaxKind::Keyword(ref s) if s == "keyword"));

        // Plain symbols still work
        let result = lex_and_parse("list").unwrap();
        assert!(matches!(result.kind, SyntaxKind::Symbol(ref s) if s == "list"));
    }

    // Lists and vectors
    #[test]
    fn test_parse_empty_list() {
        let result = lex_and_parse("()").unwrap();
        assert!(matches!(result.kind, SyntaxKind::List(ref items) if items.is_empty()));
    }

    #[test]
    fn test_parse_simple_list() {
        let result = lex_and_parse("(1 2 3)").unwrap();
        match result.kind {
            SyntaxKind::List(ref items) => {
                assert_eq!(items.len(), 3);
                assert!(matches!(items[0].kind, SyntaxKind::Int(1)));
                assert!(matches!(items[1].kind, SyntaxKind::Int(2)));
                assert!(matches!(items[2].kind, SyntaxKind::Int(3)));
            }
            _ => panic!("Expected list"),
        }
    }

    #[test]
    fn test_parse_empty_tuple() {
        let result = lex_and_parse("[]").unwrap();
        assert!(matches!(result.kind, SyntaxKind::Tuple(ref items) if items.is_empty()));
    }

    #[test]
    fn test_parse_simple_tuple() {
        let result = lex_and_parse("[1 2 3]").unwrap();
        match result.kind {
            SyntaxKind::Tuple(ref items) => {
                assert_eq!(items.len(), 3);
                assert!(matches!(items[0].kind, SyntaxKind::Int(1)));
                assert!(matches!(items[1].kind, SyntaxKind::Int(2)));
                assert!(matches!(items[2].kind, SyntaxKind::Int(3)));
            }
            _ => panic!("Expected tuple"),
        }
    }

    #[test]
    fn test_parse_empty_array() {
        let result = lex_and_parse("@[]").unwrap();
        assert!(matches!(result.kind, SyntaxKind::Array(ref items) if items.is_empty()));
    }

    #[test]
    fn test_parse_simple_array() {
        let result = lex_and_parse("@[1 2 3]").unwrap();
        match result.kind {
            SyntaxKind::Array(ref items) => {
                assert_eq!(items.len(), 3);
                assert!(matches!(items[0].kind, SyntaxKind::Int(1)));
                assert!(matches!(items[1].kind, SyntaxKind::Int(2)));
                assert!(matches!(items[2].kind, SyntaxKind::Int(3)));
            }
            _ => panic!("Expected array"),
        }
    }

    // Nested structures
    #[test]
    fn test_parse_nested_list() {
        let result = lex_and_parse("(1 (2 3) 4)").unwrap();
        match result.kind {
            SyntaxKind::List(ref items) => {
                assert_eq!(items.len(), 3);
                assert!(matches!(items[0].kind, SyntaxKind::Int(1)));
                match items[1].kind {
                    SyntaxKind::List(ref inner) => {
                        assert_eq!(inner.len(), 2);
                        assert!(matches!(inner[0].kind, SyntaxKind::Int(2)));
                        assert!(matches!(inner[1].kind, SyntaxKind::Int(3)));
                    }
                    _ => panic!("Expected nested list"),
                }
                assert!(matches!(items[2].kind, SyntaxKind::Int(4)));
            }
            _ => panic!("Expected list"),
        }
    }

    #[test]
    fn test_parse_list_with_tuple() {
        let result = lex_and_parse("(1 [2 3] 4)").unwrap();
        match result.kind {
            SyntaxKind::List(ref items) => {
                assert_eq!(items.len(), 3);
                assert!(matches!(items[0].kind, SyntaxKind::Int(1)));
                assert!(matches!(items[1].kind, SyntaxKind::Tuple(_)));
                assert!(matches!(items[2].kind, SyntaxKind::Int(4)));
            }
            _ => panic!("Expected list"),
        }
    }

    #[test]
    fn test_parse_list_with_array() {
        let result = lex_and_parse("(1 @[2 3] 4)").unwrap();
        match result.kind {
            SyntaxKind::List(ref items) => {
                assert_eq!(items.len(), 3);
                assert!(matches!(items[0].kind, SyntaxKind::Int(1)));
                assert!(matches!(items[1].kind, SyntaxKind::Array(_)));
                assert!(matches!(items[2].kind, SyntaxKind::Int(4)));
            }
            _ => panic!("Expected list"),
        }
    }

    // Quote forms
    #[test]
    fn test_parse_quote() {
        let result = lex_and_parse("'x").unwrap();
        match result.kind {
            SyntaxKind::Quote(ref inner) => {
                assert!(matches!(inner.kind, SyntaxKind::Symbol(ref s) if s == "x"));
            }
            _ => panic!("Expected quote"),
        }
    }

    #[test]
    fn test_parse_quasiquote() {
        let result = lex_and_parse("`x").unwrap();
        match result.kind {
            SyntaxKind::Quasiquote(ref inner) => {
                assert!(matches!(inner.kind, SyntaxKind::Symbol(ref s) if s == "x"));
            }
            _ => panic!("Expected quasiquote"),
        }
    }

    #[test]
    fn test_parse_unquote() {
        let result = lex_and_parse(",x").unwrap();
        match result.kind {
            SyntaxKind::Unquote(ref inner) => {
                assert!(matches!(inner.kind, SyntaxKind::Symbol(ref s) if s == "x"));
            }
            _ => panic!("Expected unquote"),
        }
    }

    #[test]
    fn test_parse_unquote_splicing() {
        let result = lex_and_parse(",@x").unwrap();
        match result.kind {
            SyntaxKind::UnquoteSplicing(ref inner) => {
                assert!(matches!(inner.kind, SyntaxKind::Symbol(ref s) if s == "x"));
            }
            _ => panic!("Expected unquote-splicing"),
        }
    }

    #[test]
    fn test_parse_quote_list() {
        let result = lex_and_parse("'(1 2 3)").unwrap();
        match result.kind {
            SyntaxKind::Quote(ref inner) => {
                assert!(matches!(inner.kind, SyntaxKind::List(ref items) if items.len() == 3));
            }
            _ => panic!("Expected quote"),
        }
    }

    // Struct/Table forms
    #[test]
    fn test_parse_struct() {
        let result = lex_and_parse("{:a 1 :b 2}").unwrap();
        match result.kind {
            SyntaxKind::Struct(ref items) => {
                assert_eq!(items.len(), 4); // 2 keyword-value pairs
                assert!(matches!(items[0].kind, SyntaxKind::Keyword(ref k) if k == "a"));
                assert!(matches!(items[1].kind, SyntaxKind::Int(1)));
                assert!(matches!(items[2].kind, SyntaxKind::Keyword(ref k) if k == "b"));
                assert!(matches!(items[3].kind, SyntaxKind::Int(2)));
            }
            _ => panic!("Expected struct"),
        }
    }

    #[test]
    fn test_parse_table() {
        let result = lex_and_parse("@{:a 1 :b 2}").unwrap();
        match result.kind {
            SyntaxKind::Table(ref items) => {
                assert_eq!(items.len(), 4); // 2 keyword-value pairs
                assert!(matches!(items[0].kind, SyntaxKind::Keyword(ref k) if k == "a"));
                assert!(matches!(items[1].kind, SyntaxKind::Int(1)));
                assert!(matches!(items[2].kind, SyntaxKind::Keyword(ref k) if k == "b"));
                assert!(matches!(items[3].kind, SyntaxKind::Int(2)));
            }
            _ => panic!("Expected table"),
        }
    }

    // Old sugar forms tests (for backwards compatibility check)
    #[test]
    fn test_parse_array_sugar() {
        let result = lex_and_parse("@[1 2 3]").unwrap();
        match result.kind {
            SyntaxKind::Array(ref items) => {
                assert_eq!(items.len(), 3);
                assert!(matches!(items[0].kind, SyntaxKind::Int(1)));
                assert!(matches!(items[1].kind, SyntaxKind::Int(2)));
                assert!(matches!(items[2].kind, SyntaxKind::Int(3)));
            }
            _ => panic!("Expected array"),
        }
    }

    #[test]
    fn test_parse_table_sugar() {
        let result = lex_and_parse("@{:a 1 :b 2}").unwrap();
        match result.kind {
            SyntaxKind::Table(ref items) => {
                assert_eq!(items.len(), 4); // 2 keyword-value pairs
                assert!(matches!(items[0].kind, SyntaxKind::Keyword(ref k) if k == "a"));
                assert!(matches!(items[1].kind, SyntaxKind::Int(1)));
                assert!(matches!(items[2].kind, SyntaxKind::Keyword(ref k) if k == "b"));
                assert!(matches!(items[3].kind, SyntaxKind::Int(2)));
            }
            _ => panic!("Expected table"),
        }
    }

    // Error cases
    #[test]
    fn test_unclosed_paren() {
        let result = lex_and_parse("(1 2 3");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unterminated list"));
    }

    #[test]
    fn test_unclosed_bracket() {
        let result = lex_and_parse("[1 2 3");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unterminated tuple"));
    }

    #[test]
    fn test_unclosed_brace() {
        let result = lex_and_parse("{:a 1");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unterminated struct"));
    }

    #[test]
    fn test_unexpected_closing_paren() {
        let result = lex_and_parse(")");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("unexpected closing parenthesis"));
    }

    #[test]
    fn test_unexpected_closing_bracket() {
        let result = lex_and_parse("]");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unexpected closing bracket"));
    }

    #[test]
    fn test_unexpected_closing_brace() {
        let result = lex_and_parse("}");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unexpected closing brace"));
    }

    #[test]
    fn test_parse_buffer_literal() {
        let result = lex_and_parse(r#"@"hello""#).unwrap();
        match result.kind {
            SyntaxKind::List(ref items) => {
                assert_eq!(items.len(), 2);
                assert!(
                    matches!(items[0].kind, SyntaxKind::Symbol(ref s) if s == "string->buffer")
                );
                assert!(matches!(items[1].kind, SyntaxKind::String(ref s) if s == "hello"));
            }
            _ => panic!("Expected list (string->buffer \"hello\")"),
        }
    }

    #[test]
    fn test_parse_buffer_literal_empty() {
        let result = lex_and_parse(r#"@"""#).unwrap();
        match result.kind {
            SyntaxKind::List(ref items) => {
                assert_eq!(items.len(), 2);
                assert!(
                    matches!(items[0].kind, SyntaxKind::Symbol(ref s) if s == "string->buffer")
                );
                assert!(matches!(items[1].kind, SyntaxKind::String(ref s) if s.is_empty()));
            }
            _ => panic!("Expected list (string->buffer \"\")"),
        }
    }

    #[test]
    fn test_list_sugar_invalid() {
        let result = lex_and_parse("@foo");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("@ must be followed by"));
    }

    // Span preservation
    #[test]
    fn test_span_simple_int() {
        let result = lex_and_parse("42").unwrap();
        assert_eq!(result.span.line, 1);
        assert_eq!(result.span.col, 1);
    }

    #[test]
    fn test_span_list() {
        let result = lex_and_parse("(1 2 3)").unwrap();
        assert_eq!(result.span.line, 1);
        // Span should cover the entire list
        assert!(result.span.end > result.span.start);
    }

    #[test]
    fn test_span_nested() {
        let result = lex_and_parse("(1 (2 3) 4)").unwrap();
        match result.kind {
            SyntaxKind::List(ref items) => {
                // Inner list should have its own span
                match items[1].kind {
                    SyntaxKind::List(_) => {
                        assert!(items[1].span.end > items[1].span.start);
                    }
                    _ => panic!("Expected nested list"),
                }
            }
            _ => panic!("Expected list"),
        }
    }

    #[test]
    fn test_read_all() {
        let result = lex_and_parse_all("1 2 3").unwrap();
        assert_eq!(result.len(), 3);
        assert!(matches!(result[0].kind, SyntaxKind::Int(1)));
        assert!(matches!(result[1].kind, SyntaxKind::Int(2)));
        assert!(matches!(result[2].kind, SyntaxKind::Int(3)));
    }

    #[test]
    fn test_read_all_mixed() {
        let result = lex_and_parse_all("42 foo (1 2) [3 4]").unwrap();
        assert_eq!(result.len(), 4);
        assert!(matches!(result[0].kind, SyntaxKind::Int(42)));
        assert!(matches!(result[1].kind, SyntaxKind::Symbol(_)));
        assert!(matches!(result[2].kind, SyntaxKind::List(_)));
        assert!(matches!(result[3].kind, SyntaxKind::Tuple(_)));
    }

    #[test]
    fn test_scopes_empty() {
        let result = lex_and_parse("foo").unwrap();
        assert_eq!(result.scopes.len(), 0);
    }
}
