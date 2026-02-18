use super::token::{SourceLoc, Token, TokenWithLoc};

/// Fast delimiter check - O(1) instead of string contains O(n)
/// Checks if a character is a Lisp delimiter
#[inline]
fn is_delimiter(c: char) -> bool {
    matches!(
        c,
        '(' | ')' | '[' | ']' | '{' | '}' | '\'' | '`' | ',' | ':' | '@'
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

    fn current(&self) -> Option<char> {
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

    fn advance(&mut self) -> Option<char> {
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

    fn peek(&self, offset: usize) -> Option<char> {
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
            } else if c == ';' {
                // Skip comment until newline
                while let Some(c) = self.advance() {
                    if c == '\n' {
                        break;
                    }
                }
            } else {
                break;
            }
        }
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

    fn read_number(&mut self) -> Result<Token<'a>, String> {
        let mut num = String::new();
        let mut has_dot = false;

        while let Some(c) = self.current() {
            if c.is_ascii_digit() || c == '-' || c == '+' {
                num.push(c);
                self.advance();
            } else if c == '.' && !has_dot {
                has_dot = true;
                num.push(c);
                self.advance();
            } else {
                break;
            }
        }

        if has_dot {
            num.parse::<f64>()
                .map(Token::Float)
                .map_err(|_| format!("Invalid float: {}", num))
        } else {
            num.parse::<i64>()
                .map(Token::Integer)
                .map_err(|_| format!("Invalid integer: {}", num))
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

        match self.current() {
            None => Ok(None),
            Some('(') => {
                self.advance();
                Ok(Some(TokenWithLoc {
                    token: Token::LeftParen,
                    loc,
                }))
            }
            Some(')') => {
                self.advance();
                Ok(Some(TokenWithLoc {
                    token: Token::RightParen,
                    loc,
                }))
            }
            Some('[') => {
                self.advance();
                Ok(Some(TokenWithLoc {
                    token: Token::LeftBracket,
                    loc,
                }))
            }
            Some(']') => {
                self.advance();
                Ok(Some(TokenWithLoc {
                    token: Token::RightBracket,
                    loc,
                }))
            }
            Some('{') => {
                self.advance();
                Ok(Some(TokenWithLoc {
                    token: Token::LeftBrace,
                    loc,
                }))
            }
            Some('}') => {
                self.advance();
                Ok(Some(TokenWithLoc {
                    token: Token::RightBrace,
                    loc,
                }))
            }
            Some('\'') => {
                self.advance();
                Ok(Some(TokenWithLoc {
                    token: Token::Quote,
                    loc,
                }))
            }
            Some('`') => {
                self.advance();
                Ok(Some(TokenWithLoc {
                    token: Token::Quasiquote,
                    loc,
                }))
            }
            Some(',') => {
                self.advance();
                if self.current() == Some('@') {
                    self.advance();
                    Ok(Some(TokenWithLoc {
                        token: Token::UnquoteSplicing,
                        loc,
                    }))
                } else {
                    Ok(Some(TokenWithLoc {
                        token: Token::Unquote,
                        loc,
                    }))
                }
            }
            Some('@') => {
                self.advance();
                Ok(Some(TokenWithLoc {
                    token: Token::ListSugar,
                    loc,
                }))
            }
            Some(':') => {
                self.advance();
                // Read keyword - must be followed by symbol characters
                let (start, end) = self.read_symbol();
                if start == end {
                    Err("Invalid keyword: expected symbol after :".to_string())
                } else {
                    let keyword = self.slice(start, end);
                    Ok(Some(TokenWithLoc {
                        token: Token::Keyword(keyword),
                        loc,
                    }))
                }
            }
            Some('"') => self.read_string().map(|s| {
                Some(TokenWithLoc {
                    token: Token::String(s),
                    loc,
                })
            }),
            Some(c) if c.is_ascii_digit() || c == '-' || c == '+' => {
                // Check if it's a number or symbol
                if let Some(next) = self.peek(1) {
                    if (c == '-' || c == '+') && !next.is_ascii_digit() {
                        let (start, end) = self.read_symbol();
                        let sym = self.slice(start, end);
                        Ok(Some(TokenWithLoc {
                            token: Token::Symbol(sym),
                            loc,
                        }))
                    } else {
                        self.read_number()
                            .map(|t| Some(TokenWithLoc { token: t, loc }))
                    }
                } else if c == '-' || c == '+' {
                    let (start, end) = self.read_symbol();
                    let sym = self.slice(start, end);
                    Ok(Some(TokenWithLoc {
                        token: Token::Symbol(sym),
                        loc,
                    }))
                } else {
                    self.read_number()
                        .map(|t| Some(TokenWithLoc { token: t, loc }))
                }
            }
            Some('#') => {
                self.advance();
                match self.current() {
                    Some('t') => {
                        self.advance();
                        Ok(Some(TokenWithLoc {
                            token: Token::Bool(true),
                            loc,
                        }))
                    }
                    Some('f') => {
                        self.advance();
                        Ok(Some(TokenWithLoc {
                            token: Token::Bool(false),
                            loc,
                        }))
                    }
                    _ => Err("Invalid # syntax".to_string()),
                }
            }
            Some(_) => {
                let (start, end) = self.read_symbol();
                let sym = self.slice(start, end);
                if sym == "nil" {
                    Ok(Some(TokenWithLoc {
                        token: Token::Nil,
                        loc,
                    }))
                } else {
                    Ok(Some(TokenWithLoc {
                        token: Token::Symbol(sym),
                        loc,
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
