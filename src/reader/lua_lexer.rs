//! Lua-surface-syntax tokenizer.
//!
//! Produces `LuaToken` values with source locations. Numbers reuse the
//! patterns from `numeric.rs` (decimal, hex, scientific notation).

use super::token::SourceLoc;

/// Token types for the Lua surface syntax.
#[derive(Debug, Clone, PartialEq)]
pub enum LuaToken {
    // Literals
    Int(i64),
    Float(f64),
    String(String),
    True,
    False,
    Nil,

    // Identifiers
    Ident(String),

    // Varargs
    DotDotDot,

    // Keywords
    Function,
    End,
    If,
    Then,
    Else,
    ElseIf,
    While,
    Do,
    For,
    In,
    Local,
    Return,
    And,
    Or,
    Not,
    Break,
    Repeat,
    Until,

    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    Eq,
    Neq,
    Lt,
    Gt,
    Le,
    Ge,
    Assign,
    DotDot,
    Hash,
    Dot,
    Colon,

    // Delimiters
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Comma,
    Semicolon,

    // Special
    Backtick,
    Eof,
}

/// A token with its source location and byte length.
#[derive(Debug, Clone)]
pub struct LuaTokenLoc {
    pub token: LuaToken,
    pub loc: SourceLoc,
    pub len: usize,
}

pub struct LuaLexer {
    input: Vec<char>,
    pos: usize,
    line: usize,
    col: usize,
    file: String,
}

impl LuaLexer {
    pub fn new(input: &str, file: &str) -> Self {
        LuaLexer {
            input: input.chars().collect(),
            pos: 0,
            line: 1,
            col: 1,
            file: file.to_string(),
        }
    }

    fn loc(&self) -> SourceLoc {
        SourceLoc::new(&self.file, self.line, self.col)
    }

    fn peek(&self) -> Option<char> {
        self.input.get(self.pos).copied()
    }

    fn peek2(&self) -> Option<char> {
        self.input.get(self.pos + 1).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += 1;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn skip_whitespace(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_ascii_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn skip_line_comment(&mut self) {
        while let Some(c) = self.advance() {
            if c == '\n' {
                break;
            }
        }
    }

    fn skip_block_comment(&mut self, level: usize) -> Result<(), String> {
        // We've already consumed `--[=*[`
        let start_line = self.line;
        loop {
            match self.advance() {
                None => {
                    return Err(format!(
                        "{}:{}:{}: unterminated block comment starting at line {}",
                        self.file, self.line, self.col, start_line
                    ));
                }
                Some(']') => {
                    let mut eq_count = 0;
                    while self.peek() == Some('=') {
                        eq_count += 1;
                        self.advance();
                    }
                    if eq_count == level && self.peek() == Some(']') {
                        self.advance();
                        return Ok(());
                    }
                }
                _ => {}
            }
        }
    }

    fn read_string(&mut self, quote: char) -> Result<String, String> {
        let start_loc = self.loc();
        self.advance(); // skip opening quote
        let mut s = std::string::String::new();
        loop {
            match self.advance() {
                None => {
                    return Err(format!("{}: unterminated string", start_loc.position()));
                }
                Some('\\') => match self.advance() {
                    None => {
                        return Err(format!(
                            "{}: unterminated string escape",
                            start_loc.position()
                        ));
                    }
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    Some('r') => s.push('\r'),
                    Some('a') => s.push('\x07'), // bell
                    Some('b') => s.push('\x08'), // backspace
                    Some('f') => s.push('\x0C'), // form feed
                    Some('v') => s.push('\x0B'), // vertical tab
                    Some('\\') => s.push('\\'),
                    Some('\'') => s.push('\''),
                    Some('"') => s.push('"'),
                    Some('0') => s.push('\0'),
                    Some('x') => {
                        // \xNN hex escape
                        let mut hex = String::new();
                        for _ in 0..2 {
                            match self.advance() {
                                Some(c) if c.is_ascii_hexdigit() => hex.push(c),
                                _ => {
                                    return Err(format!(
                                        "{}: invalid \\x escape",
                                        start_loc.position()
                                    ))
                                }
                            }
                        }
                        let val = u8::from_str_radix(&hex, 16).unwrap();
                        s.push(val as char);
                    }
                    Some(c) if c.is_ascii_digit() => {
                        // \ddd decimal escape (1-3 digits)
                        let mut digits = String::new();
                        digits.push(c);
                        for _ in 0..2 {
                            if let Some(d) = self.peek() {
                                if d.is_ascii_digit() {
                                    digits.push(d);
                                    self.advance();
                                } else {
                                    break;
                                }
                            }
                        }
                        let val: u32 = digits.parse().unwrap();
                        if val > 255 {
                            return Err(format!(
                                "{}: decimal escape too large: \\{}",
                                start_loc.position(),
                                digits
                            ));
                        }
                        s.push(char::from(val as u8));
                    }
                    Some(c) => {
                        return Err(format!(
                            "{}: unknown escape sequence \\{}",
                            start_loc.position(),
                            c
                        ));
                    }
                },
                Some(c) if c == quote => return Ok(s),
                Some(c) => s.push(c),
            }
        }
    }

    /// Count `=` signs after current `[` and consume them + the closing `[`.
    /// Returns the level (number of `=` signs).
    fn read_long_string_open(&mut self) -> usize {
        // We've already consumed the first `[`
        let mut level = 0;
        while self.peek() == Some('=') {
            self.advance();
            level += 1;
        }
        self.advance(); // skip closing `[`
        level
    }

    fn read_long_string(&mut self, level: usize) -> Result<String, String> {
        let start_loc = self.loc();
        let mut s = std::string::String::new();
        // Skip optional leading newline
        if self.peek() == Some('\n') {
            self.advance();
        }
        loop {
            match self.advance() {
                None => {
                    return Err(format!(
                        "{}: unterminated long string",
                        start_loc.position()
                    ));
                }
                Some(']') => {
                    // Check for `]=*]` with matching level
                    let mut eq_count = 0;
                    while self.peek() == Some('=') {
                        eq_count += 1;
                        self.advance();
                    }
                    if eq_count == level && self.peek() == Some(']') {
                        self.advance();
                        return Ok(s);
                    }
                    // Not a match — push what we consumed
                    s.push(']');
                    for _ in 0..eq_count {
                        s.push('=');
                    }
                }
                Some(c) => s.push(c),
            }
        }
    }

    fn read_number(&mut self) -> Result<LuaToken, String> {
        let start = self.pos;
        let mut is_float = false;

        // Hex literal
        if self.peek() == Some('0') && matches!(self.peek2(), Some('x') | Some('X')) {
            self.advance(); // 0
            self.advance(); // x
            while let Some(c) = self.peek() {
                if c.is_ascii_hexdigit() || c == '_' {
                    self.advance();
                } else {
                    break;
                }
            }
            let s: String = self.input[start..self.pos]
                .iter()
                .filter(|c| **c != '_')
                .collect();
            let val =
                i64::from_str_radix(&s[2..], 16).map_err(|e| format!("bad hex literal: {}", e))?;
            return Ok(LuaToken::Int(val));
        }

        // Decimal integer or float
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == '_' {
                self.advance();
            } else {
                break;
            }
        }

        // Fractional part
        if self.peek() == Some('.') && self.peek2().is_some_and(|c| c.is_ascii_digit()) {
            is_float = true;
            self.advance(); // .
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() || c == '_' {
                    self.advance();
                } else {
                    break;
                }
            }
        }

        // Exponent
        if matches!(self.peek(), Some('e') | Some('E')) {
            is_float = true;
            self.advance();
            if matches!(self.peek(), Some('+') | Some('-')) {
                self.advance();
            }
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    self.advance();
                } else {
                    break;
                }
            }
        }

        let s: String = self.input[start..self.pos]
            .iter()
            .filter(|c| **c != '_')
            .collect();

        if is_float {
            let val: f64 = s.parse().map_err(|e| format!("bad float literal: {}", e))?;
            Ok(LuaToken::Float(val))
        } else {
            let val: i64 = s
                .parse()
                .map_err(|e| format!("bad integer literal: {}", e))?;
            Ok(LuaToken::Int(val))
        }
    }

    fn read_ident(&mut self) -> String {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' {
                self.advance();
            } else {
                break;
            }
        }
        self.input[start..self.pos].iter().collect()
    }

    fn keyword_or_ident(&self, s: &str) -> LuaToken {
        match s {
            "function" => LuaToken::Function,
            "end" => LuaToken::End,
            "if" => LuaToken::If,
            "then" => LuaToken::Then,
            "else" => LuaToken::Else,
            "elseif" => LuaToken::ElseIf,
            "while" => LuaToken::While,
            "do" => LuaToken::Do,
            "for" => LuaToken::For,
            "in" => LuaToken::In,
            "local" => LuaToken::Local,
            "return" => LuaToken::Return,
            "and" => LuaToken::And,
            "or" => LuaToken::Or,
            "not" => LuaToken::Not,
            "break" => LuaToken::Break,
            "repeat" => LuaToken::Repeat,
            "until" => LuaToken::Until,
            "true" => LuaToken::True,
            "false" => LuaToken::False,
            "nil" => LuaToken::Nil,
            _ => LuaToken::Ident(s.to_string()),
        }
    }

    /// Tokenize the entire input, returning all tokens with locations.
    pub fn tokenize(&mut self) -> Result<Vec<LuaTokenLoc>, String> {
        let mut tokens = Vec::new();
        loop {
            self.skip_whitespace();
            let loc = self.loc();
            let start_pos = self.pos;

            let c = match self.peek() {
                None => {
                    tokens.push(LuaTokenLoc {
                        token: LuaToken::Eof,
                        loc,
                        len: 0,
                    });
                    return Ok(tokens);
                }
                Some(c) => c,
            };

            let token = match c {
                '-' if self.peek2() == Some('-') => {
                    self.advance();
                    self.advance();
                    // Check for block comment --[=*[ ... ]=*]
                    if self.peek() == Some('[')
                        && (self.input.get(self.pos + 1) == Some(&'[')
                            || self.input.get(self.pos + 1) == Some(&'='))
                    {
                        self.advance(); // [
                        let level = self.read_long_string_open();
                        self.skip_block_comment(level)?;
                    } else {
                        self.skip_line_comment();
                    }
                    continue;
                }

                // String literals
                '"' | '\'' => {
                    let s = self.read_string(c)?;
                    LuaToken::String(s)
                }

                // Long strings [[ ... ]], [=[ ... ]=], [==[ ... ]==], etc.
                '[' if self.peek2() == Some('[') || self.peek2() == Some('=') => {
                    self.advance(); // first [
                    let level = self.read_long_string_open();
                    let s = self.read_long_string(level)?;
                    LuaToken::String(s)
                }

                // Numbers
                '0'..='9' => self.read_number()?,

                // Identifiers and keywords
                c if c.is_alphabetic() || c == '_' => {
                    let name = self.read_ident();
                    self.keyword_or_ident(&name)
                }

                // Two-char operators
                '~' if self.peek2() == Some('=') => {
                    self.advance();
                    self.advance();
                    LuaToken::Neq
                }
                '<' if self.peek2() == Some('=') => {
                    self.advance();
                    self.advance();
                    LuaToken::Le
                }
                '>' if self.peek2() == Some('=') => {
                    self.advance();
                    self.advance();
                    LuaToken::Ge
                }
                '=' if self.peek2() == Some('=') => {
                    self.advance();
                    self.advance();
                    LuaToken::Eq
                }
                '.' if self.peek2() == Some('.') => {
                    self.advance();
                    self.advance();
                    // Check for ... (varargs) vs .. (concat)
                    if self.peek() == Some('.') {
                        self.advance();
                        LuaToken::DotDotDot
                    } else {
                        LuaToken::DotDot
                    }
                }

                // Single-char operators and delimiters
                '+' => {
                    self.advance();
                    LuaToken::Plus
                }
                '-' => {
                    self.advance();
                    LuaToken::Minus
                }
                '*' => {
                    self.advance();
                    LuaToken::Star
                }
                '/' => {
                    self.advance();
                    LuaToken::Slash
                }
                '%' => {
                    self.advance();
                    LuaToken::Percent
                }
                '^' => {
                    self.advance();
                    LuaToken::Caret
                }
                '<' => {
                    self.advance();
                    LuaToken::Lt
                }
                '>' => {
                    self.advance();
                    LuaToken::Gt
                }
                '=' => {
                    self.advance();
                    LuaToken::Assign
                }
                '#' => {
                    self.advance();
                    LuaToken::Hash
                }
                '.' => {
                    self.advance();
                    LuaToken::Dot
                }
                ':' => {
                    self.advance();
                    LuaToken::Colon
                }
                '(' => {
                    self.advance();
                    LuaToken::LParen
                }
                ')' => {
                    self.advance();
                    LuaToken::RParen
                }
                '[' => {
                    self.advance();
                    LuaToken::LBracket
                }
                ']' => {
                    self.advance();
                    LuaToken::RBracket
                }
                '{' => {
                    self.advance();
                    LuaToken::LBrace
                }
                '}' => {
                    self.advance();
                    LuaToken::RBrace
                }
                ',' => {
                    self.advance();
                    LuaToken::Comma
                }
                ';' => {
                    self.advance();
                    LuaToken::Semicolon
                }
                '`' => {
                    self.advance();
                    LuaToken::Backtick
                }

                _ => {
                    return Err(format!(
                        "{}:{}:{}: unexpected character '{}'",
                        self.file, self.line, self.col, c
                    ));
                }
            };

            let len = self.pos - start_pos;
            tokens.push(LuaTokenLoc { token, loc, len });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(input: &str) -> Vec<LuaToken> {
        let mut lexer = LuaLexer::new(input, "<test>");
        lexer
            .tokenize()
            .unwrap()
            .into_iter()
            .map(|t| t.token)
            .collect()
    }

    #[test]
    fn test_basic_tokens() {
        let tokens = lex("local x = 42");
        assert_eq!(
            tokens,
            vec![
                LuaToken::Local,
                LuaToken::Ident("x".into()),
                LuaToken::Assign,
                LuaToken::Int(42),
                LuaToken::Eof
            ]
        );
    }

    #[test]
    fn test_strings() {
        let tokens = lex(r#""hello" 'world'"#);
        assert_eq!(
            tokens,
            vec![
                LuaToken::String("hello".into()),
                LuaToken::String("world".into()),
                LuaToken::Eof
            ]
        );
    }

    #[test]
    fn test_comments() {
        let tokens = lex("x -- comment\ny");
        assert_eq!(
            tokens,
            vec![
                LuaToken::Ident("x".into()),
                LuaToken::Ident("y".into()),
                LuaToken::Eof
            ]
        );
    }

    #[test]
    fn test_block_comment() {
        let tokens = lex("x --[[ block\ncomment ]] y");
        assert_eq!(
            tokens,
            vec![
                LuaToken::Ident("x".into()),
                LuaToken::Ident("y".into()),
                LuaToken::Eof
            ]
        );
    }

    #[test]
    fn test_operators() {
        let tokens = lex("~= <= >= == ..");
        assert_eq!(
            tokens,
            vec![
                LuaToken::Neq,
                LuaToken::Le,
                LuaToken::Ge,
                LuaToken::Eq,
                LuaToken::DotDot,
                LuaToken::Eof
            ]
        );
    }

    #[test]
    fn test_long_string() {
        let tokens = lex("[[hello\nworld]]");
        assert_eq!(
            tokens,
            vec![LuaToken::String("hello\nworld".into()), LuaToken::Eof]
        );
    }

    #[test]
    #[allow(clippy::approx_constant)]
    fn test_float() {
        let tokens = lex("3.14 1e10");
        assert_eq!(
            tokens,
            vec![LuaToken::Float(3.14), LuaToken::Float(1e10), LuaToken::Eof]
        );
    }

    #[test]
    fn test_hex() {
        let tokens = lex("0xFF");
        assert_eq!(tokens, vec![LuaToken::Int(255), LuaToken::Eof]);
    }
}
