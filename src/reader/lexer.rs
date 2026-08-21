use super::token::{SourceLoc, Token, TokenWithLoc, UNKNOWN_FILE};

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
        Lexer::with_file(input, UNKNOWN_FILE)
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

    /// Bundle `token` with the span running from `start_pos` to the cursor's
    /// current position. Every arm of `next_token_with_loc` captures `loc` and
    /// `start_pos` before consuming the lexeme and then calls this once it has
    /// advanced past it, so the `len = pos - start_pos`, `byte_offset =
    /// start_pos` relationship lives here rather than being re-derived (and
    /// re-typo'd) at each of the ~28 token sites.
    fn spanned(&self, token: Token<'a>, loc: SourceLoc, start_pos: usize) -> TokenWithLoc<'a> {
        TokenWithLoc::new(token, loc, self.pos - start_pos, start_pos)
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
                Ok(Some(self.spanned(Token::Comment(text), loc, start_pos)))
            }
            Some('(') => {
                self.advance();
                Ok(Some(self.spanned(Token::LeftParen, loc, start_pos)))
            }
            Some(')') => {
                self.advance();
                Ok(Some(self.spanned(Token::RightParen, loc, start_pos)))
            }
            Some('[') => {
                self.advance();
                Ok(Some(self.spanned(Token::LeftBracket, loc, start_pos)))
            }
            Some(']') => {
                self.advance();
                Ok(Some(self.spanned(Token::RightBracket, loc, start_pos)))
            }
            Some('{') => {
                self.advance();
                Ok(Some(self.spanned(Token::LeftBrace, loc, start_pos)))
            }
            Some('}') => {
                self.advance();
                Ok(Some(self.spanned(Token::RightBrace, loc, start_pos)))
            }
            Some('\'') => {
                self.advance();
                Ok(Some(self.spanned(Token::Quote, loc, start_pos)))
            }
            Some('`') => {
                self.advance();
                Ok(Some(self.spanned(Token::Quasiquote, loc, start_pos)))
            }
            Some(',') => {
                self.advance();
                if self.current() == Some(';') {
                    self.advance();
                    Ok(Some(self.spanned(Token::UnquoteSplicing, loc, start_pos)))
                } else {
                    Ok(Some(self.spanned(Token::Unquote, loc, start_pos)))
                }
            }
            Some(';') => {
                self.advance();
                Ok(Some(self.spanned(Token::Splice, loc, start_pos)))
            }
            Some('|') => {
                self.advance();
                Ok(Some(self.spanned(Token::Pipe, loc, start_pos)))
            }
            Some('@') => {
                self.advance();
                match self.current() {
                    // @| → mutable set literal delimiter
                    Some('|') => {
                        self.advance();
                        Ok(Some(self.spanned(Token::AtPipe, loc, start_pos)))
                    }
                    // @b[ → mutable bytes literal
                    Some('b') if self.peek(1) == Some('[') => {
                        self.advance(); // consume 'b'
                        self.advance(); // consume '['
                        Ok(Some(self.spanned(Token::AtBytesBracket, loc, start_pos)))
                    }
                    // @symbol → symbol with @ prefix (e.g. @set, @array)
                    Some(c) if is_symbol_start(c) => {
                        let (_, end) = self.read_symbol();
                        let name = self.slice(start_pos, end);
                        Ok(Some(self.spanned(Token::Symbol(name), loc, start_pos)))
                    }
                    // @[, @{, @" → collection sugar
                    _ => Ok(Some(self.spanned(Token::ListSugar, loc, start_pos))),
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
                    Ok(Some(self.spanned(Token::Keyword(keyword), loc, start_pos)))
                }
            }
            Some('"') => {
                let token = Token::String(self.read_string()?);
                Ok(Some(self.spanned(token, loc, start_pos)))
            }
            Some(c) if c.is_ascii_digit() || c == '-' || c == '+' => {
                // Check if it's a number or symbol
                if let Some(next) = self.peek(1) {
                    if (c == '-' || c == '+') && !next.is_ascii_digit() {
                        let (start, end) = self.read_symbol();
                        let sym = self.slice(start, end);
                        Ok(Some(self.spanned(Token::Symbol(sym), loc, start_pos)))
                    } else {
                        let token = self.read_number()?;
                        Ok(Some(self.spanned(token, loc, start_pos)))
                    }
                } else if c == '-' || c == '+' {
                    let (start, end) = self.read_symbol();
                    let sym = self.slice(start, end);
                    Ok(Some(self.spanned(Token::Symbol(sym), loc, start_pos)))
                } else {
                    let token = self.read_number()?;
                    Ok(Some(self.spanned(token, loc, start_pos)))
                }
            }

            // b[ → bytes literal
            Some('b') if self.peek(1) == Some('[') => {
                self.advance(); // consume 'b'
                self.advance(); // consume '['
                Ok(Some(self.spanned(Token::BytesBracket, loc, start_pos)))
            }

            Some(_) => {
                let (start, end) = self.read_symbol();
                let sym = self.slice(start, end);
                if sym == "nil" {
                    Ok(Some(self.spanned(Token::Nil, loc, start_pos)))
                } else if sym == "true" {
                    Ok(Some(self.spanned(Token::Bool(true), loc, start_pos)))
                } else if sym == "false" {
                    Ok(Some(self.spanned(Token::Bool(false), loc, start_pos)))
                } else {
                    Ok(Some(self.spanned(Token::Symbol(sym), loc, start_pos)))
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
mod tests;
