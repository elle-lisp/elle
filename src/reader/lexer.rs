use super::token::{SourceLoc, Token, TokenWithLoc};

/// Fast delimiter check - O(1) instead of string contains O(n)
/// Checks if a character is a Lisp delimiter
#[inline]
fn is_delimiter(c: char) -> bool {
    matches!(
        c,
        '(' | ')' | '[' | ']' | '{' | '}' | '\'' | '`' | ',' | ':' | '@' | ';' | '|'
    )
}

/// Check if a character can start a symbol name (for qualified name parsing).
/// Used to determine if `module:name` should be read as a single qualified symbol.
#[inline]
fn is_symbol_start(c: char) -> bool {
    c.is_alphabetic() || matches!(c, '_' | '-' | '+' | '*' | '/' | '!' | '?' | '<' | '>' | '=')
}

pub struct Lexer<'a> {
    input: &'a str,
    bytes: &'a [u8],
    pos: usize,
    line: usize,
    col: usize,
    file: String,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Lexer {
            input,
            bytes: input.as_bytes(),
            pos: 0,
            line: 1,
            col: 1,
            file: "<unknown>".to_string(),
        }
    }

    pub fn with_file(input: &'a str, file: impl Into<String>) -> Self {
        Lexer {
            input,
            bytes: input.as_bytes(),
            pos: 0,
            line: 1,
            col: 1,
            file: file.into(),
        }
    }

    fn get_loc(&self) -> SourceLoc {
        SourceLoc::new(&self.file, self.line, self.col)
    }

    pub(super) fn current(&self) -> Option<char> {
        if self.pos >= self.bytes.len() {
            return None;
        }
        // Decode UTF-8 character at current position
        let byte = self.bytes[self.pos];
        if byte < 128 {
            Some(byte as char)
        } else {
            // Multi-byte UTF-8 character
            self.input[self.pos..].chars().next()
        }
    }

    pub(super) fn advance(&mut self) -> Option<char> {
        let c = self.current();
        if let Some(ch) = c {
            if ch == '\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
            self.pos += ch.len_utf8();
        }
        c
    }

    pub(super) fn peek(&self, offset: usize) -> Option<char> {
        if self.pos + offset >= self.bytes.len() {
            return None;
        }
        let byte_pos = self.pos + offset;
        let byte = self.bytes[byte_pos];
        if byte < 128 {
            Some(byte as char)
        } else {
            self.input[byte_pos..].chars().next()
        }
    }

    /// Get a slice of the original input from start to current position
    fn slice(&self, start: usize, end: usize) -> &'a str {
        &self.input[start..end]
    }

    fn skip_whitespace(&mut self) {
        while let Some(c) = self.current() {
            if c.is_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    /// Read a comment starting at the current `#` character.
    /// Returns the full comment text including the `#` prefix.
    /// Leaves the lexer positioned after the newline (or at EOF).
    fn read_comment(&mut self) -> String {
        let mut text = String::new();
        // The caller guarantees current() == Some('#')
        while let Some(c) = self.advance() {
            text.push(c);
            if c == '\n' {
                break;
            }
        }
        text
    }

    fn read_string(&mut self) -> Result<String, String> {
        self.advance(); // skip opening quote
        let mut s = String::new();
        loop {
            match self.current() {
                None => return Err("Unterminated string".to_string()),
                Some('"') => {
                    self.advance();
                    return Ok(s);
                }
                Some('\\') => {
                    self.advance();
                    match self.current() {
                        Some('n') => s.push('\n'),
                        Some('t') => s.push('\t'),
                        Some('r') => s.push('\r'),
                        Some('\\') => s.push('\\'),
                        Some('"') => s.push('"'),
                        Some(c) => s.push(c),
                        None => return Err("Unterminated string escape".to_string()),
                    }
                    self.advance();
                }
                Some(c) => {
                    s.push(c);
                    self.advance();
                }
            }
        }
    }

    /// Read a symbol and return a slice of the original input.
    /// Handles qualified names like `module:name` as a single symbol.
    fn read_symbol(&mut self) -> (usize, usize) {
        let start = self.pos;
        while let Some(c) = self.current() {
            // Use fast delimiter check instead of string contains()
            if c.is_whitespace() || is_delimiter(c) {
                // Check for qualified name: if we hit ':' and next char can start a symbol,
                // continue reading as a qualified name
                if c == ':' {
                    if let Some(next) = self.peek(1) {
                        if is_symbol_start(next) {
                            // Include the colon and continue reading
                            self.advance(); // consume ':'
                            continue;
                        }
                    }
                }
                break;
            }
            self.advance();
        }
        (start, self.pos)
    }

    pub fn next_token_with_loc(&mut self) -> Result<Option<TokenWithLoc<'a>>, String> {
        self.skip_whitespace();
        let loc = self.get_loc();
        let start_pos = self.pos;

        match self.current() {
            None => Ok(None),
            Some('#') => {
                let text = self.read_comment();
                let len = self.pos - start_pos;
                Ok(Some(TokenWithLoc {
                    token: Token::Comment(text),
                    loc,
                    len,
                    byte_offset: start_pos,
                }))
            }
            Some('(') => {
                self.advance();
                Ok(Some(TokenWithLoc {
                    token: Token::LeftParen,
                    loc,
                    len: self.pos - start_pos,
                    byte_offset: start_pos,
                }))
            }
            Some(')') => {
                self.advance();
                Ok(Some(TokenWithLoc {
                    token: Token::RightParen,
                    loc,
                    len: self.pos - start_pos,
                    byte_offset: start_pos,
                }))
            }
            Some('[') => {
                self.advance();
                Ok(Some(TokenWithLoc {
                    token: Token::LeftBracket,
                    loc,
                    len: self.pos - start_pos,
                    byte_offset: start_pos,
                }))
            }
            Some(']') => {
                self.advance();
                Ok(Some(TokenWithLoc {
                    token: Token::RightBracket,
                    loc,
                    len: self.pos - start_pos,
                    byte_offset: start_pos,
                }))
            }
            Some('{') => {
                self.advance();
                Ok(Some(TokenWithLoc {
                    token: Token::LeftBrace,
                    loc,
                    len: self.pos - start_pos,
                    byte_offset: start_pos,
                }))
            }
            Some('}') => {
                self.advance();
                Ok(Some(TokenWithLoc {
                    token: Token::RightBrace,
                    loc,
                    len: self.pos - start_pos,
                    byte_offset: start_pos,
                }))
            }
            Some('\'') => {
                self.advance();
                Ok(Some(TokenWithLoc {
                    token: Token::Quote,
                    loc,
                    len: self.pos - start_pos,
                    byte_offset: start_pos,
                }))
            }
            Some('`') => {
                self.advance();
                Ok(Some(TokenWithLoc {
                    token: Token::Quasiquote,
                    loc,
                    len: self.pos - start_pos,
                    byte_offset: start_pos,
                }))
            }
            Some(',') => {
                self.advance();
                if self.current() == Some(';') {
                    self.advance();
                    Ok(Some(TokenWithLoc {
                        token: Token::UnquoteSplicing,
                        loc,
                        len: self.pos - start_pos,
                        byte_offset: start_pos,
                    }))
                } else {
                    Ok(Some(TokenWithLoc {
                        token: Token::Unquote,
                        loc,
                        len: self.pos - start_pos,
                        byte_offset: start_pos,
                    }))
                }
            }
            Some(';') => {
                self.advance();
                Ok(Some(TokenWithLoc {
                    token: Token::Splice,
                    loc,
                    len: self.pos - start_pos,
                    byte_offset: start_pos,
                }))
            }
            Some('|') => {
                self.advance();
                Ok(Some(TokenWithLoc {
                    token: Token::Pipe,
                    loc,
                    len: self.pos - start_pos,
                    byte_offset: start_pos,
                }))
            }
            Some('@') => {
                self.advance();
                match self.current() {
                    // @| → mutable set literal delimiter
                    Some('|') => {
                        self.advance();
                        Ok(Some(TokenWithLoc {
                            token: Token::AtPipe,
                            loc,
                            len: self.pos - start_pos,
                            byte_offset: start_pos,
                        }))
                    }
                    // @b[ → mutable bytes literal
                    Some('b') if self.peek(1) == Some('[') => {
                        self.advance(); // consume 'b'
                        self.advance(); // consume '['
                        Ok(Some(TokenWithLoc {
                            token: Token::AtBytesBracket,
                            loc,
                            len: self.pos - start_pos,
                            byte_offset: start_pos,
                        }))
                    }
                    // @symbol → symbol with @ prefix (e.g. @set, @array)
                    Some(c) if is_symbol_start(c) => {
                        let (_, end) = self.read_symbol();
                        let name = self.slice(start_pos, end);
                        Ok(Some(TokenWithLoc {
                            token: Token::Symbol(name),
                            loc,
                            len: self.pos - start_pos,
                            byte_offset: start_pos,
                        }))
                    }
                    // @[, @{, @" → collection sugar
                    _ => Ok(Some(TokenWithLoc {
                        token: Token::ListSugar,
                        loc,
                        len: self.pos - start_pos,
                        byte_offset: start_pos,
                    })),
                }
            }
            Some(':') => {
                self.advance();
                // Allow :@name for mutable type keywords (e.g. :@set, :@array)
                let at_prefix = if self.current() == Some('@') {
                    self.advance();
                    true
                } else {
                    false
                };
                // Read keyword - must be followed by symbol characters
                let (start, end) = self.read_symbol();
                if start == end {
                    Err("Invalid keyword: expected symbol after :".to_string())
                } else {
                    let keyword = if at_prefix {
                        // The @ was already consumed, so we need to include it in the keyword name.
                        // Since @ is at position (start - 1) in the source, we can slice from there.
                        self.slice(start - 1, end)
                    } else {
                        self.slice(start, end)
                    };
                    Ok(Some(TokenWithLoc {
                        token: Token::Keyword(keyword),
                        loc,
                        len: self.pos - start_pos,
                        byte_offset: start_pos,
                    }))
                }
            }
            Some('"') => {
                let token = Token::String(self.read_string()?);
                let len = self.pos - start_pos;
                Ok(Some(TokenWithLoc {
                    token,
                    loc,
                    len,
                    byte_offset: start_pos,
                }))
            }
            Some(c) if c.is_ascii_digit() || c == '-' || c == '+' => {
                // Check if it's a number or symbol
                if let Some(next) = self.peek(1) {
                    if (c == '-' || c == '+') && !next.is_ascii_digit() {
                        let (start, end) = self.read_symbol();
                        let sym = self.slice(start, end);
                        Ok(Some(TokenWithLoc {
                            token: Token::Symbol(sym),
                            loc,
                            len: self.pos - start_pos,
                            byte_offset: start_pos,
                        }))
                    } else {
                        let token = self.read_number()?;
                        let len = self.pos - start_pos;
                        Ok(Some(TokenWithLoc {
                            token,
                            loc,
                            len,
                            byte_offset: start_pos,
                        }))
                    }
                } else if c == '-' || c == '+' {
                    let (start, end) = self.read_symbol();
                    let sym = self.slice(start, end);
                    Ok(Some(TokenWithLoc {
                        token: Token::Symbol(sym),
                        loc,
                        len: self.pos - start_pos,
                        byte_offset: start_pos,
                    }))
                } else {
                    let token = self.read_number()?;
                    let len = self.pos - start_pos;
                    Ok(Some(TokenWithLoc {
                        token,
                        loc,
                        len,
                        byte_offset: start_pos,
                    }))
                }
            }

            // b[ → bytes literal
            Some('b') if self.peek(1) == Some('[') => {
                self.advance(); // consume 'b'
                self.advance(); // consume '['
                Ok(Some(TokenWithLoc {
                    token: Token::BytesBracket,
                    loc,
                    len: self.pos - start_pos,
                    byte_offset: start_pos,
                }))
            }

            Some(_) => {
                let (start, end) = self.read_symbol();
                let sym = self.slice(start, end);
                let len = self.pos - start_pos;
                if sym == "nil" {
                    Ok(Some(TokenWithLoc {
                        token: Token::Nil,
                        loc,
                        len,
                        byte_offset: start_pos,
                    }))
                } else if sym == "true" {
                    Ok(Some(TokenWithLoc {
                        token: Token::Bool(true),
                        loc,
                        len,
                        byte_offset: start_pos,
                    }))
                } else if sym == "false" {
                    Ok(Some(TokenWithLoc {
                        token: Token::Bool(false),
                        loc,
                        len,
                        byte_offset: start_pos,
                    }))
                } else {
                    Ok(Some(TokenWithLoc {
                        token: Token::Symbol(sym),
                        loc,
                        len,
                        byte_offset: start_pos,
                    }))
                }
            }
        }
    }

    pub fn next_token(&mut self) -> Result<Option<Token<'a>>, String> {
        self.next_token_with_loc()
            .map(|opt| opt.map(|twl| twl.token))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex_single(input: &str) -> Token<'_> {
        let mut lexer = Lexer::new(input);
        lexer.next_token().unwrap().unwrap()
    }

    #[test]
    fn true_word_lexes_as_bool() {
        assert!(matches!(lex_single("true"), Token::Bool(true)));
    }

    #[test]
    fn false_word_lexes_as_bool() {
        assert!(matches!(lex_single("false"), Token::Bool(false)));
    }

    #[test]
    fn true_question_mark_is_symbol() {
        assert!(matches!(lex_single("true?"), Token::Symbol("true?")));
    }

    #[test]
    fn trueish_is_symbol() {
        assert!(matches!(lex_single("trueish"), Token::Symbol("trueish")));
    }

    #[test]
    fn false_positive_is_symbol() {
        assert!(matches!(
            lex_single("false-positive"),
            Token::Symbol("false-positive")
        ));
    }

    #[test]
    fn truetrue_is_symbol() {
        assert!(matches!(lex_single("truetrue"), Token::Symbol("truetrue")));
    }

    #[test]
    fn comment_is_token() {
        let mut lexer = Lexer::new("# hello");
        let tok = lexer.next_token().unwrap().unwrap();
        assert!(matches!(tok, Token::Comment(s) if s == "# hello"));
    }

    #[test]
    fn doc_comment_is_token() {
        let mut lexer = Lexer::new("## doc text");
        let tok = lexer.next_token().unwrap().unwrap();
        assert!(matches!(tok, Token::Comment(s) if s == "## doc text"));
    }

    #[test]
    fn comment_before_code() {
        let mut lexer = Lexer::new("# comment\n42");
        let first = lexer.next_token().unwrap().unwrap();
        assert!(matches!(first, Token::Comment(_)));
        let second = lexer.next_token().unwrap().unwrap();
        assert!(matches!(second, Token::Integer(42)));
    }

    #[test]
    fn comment_after_code() {
        let mut lexer = Lexer::new("42 # inline comment");
        let first = lexer.next_token().unwrap().unwrap();
        assert!(matches!(first, Token::Integer(42)));
        let second = lexer.next_token().unwrap().unwrap();
        assert!(matches!(second, Token::Comment(s) if s.contains("inline comment")));
    }

    #[test]
    fn comment_at_eof() {
        let mut lexer = Lexer::new("# trailing");
        let tok = lexer.next_token().unwrap().unwrap();
        assert!(matches!(tok, Token::Comment(s) if s == "# trailing"));
        assert!(lexer.next_token().unwrap().is_none());
    }

    #[test]
    fn comment_with_special_chars() {
        let mut lexer = Lexer::new("# (parens) [brackets] 'quote");
        let tok = lexer.next_token().unwrap().unwrap();
        assert!(matches!(tok, Token::Comment(s) if s.contains("(parens)")));
    }
}
