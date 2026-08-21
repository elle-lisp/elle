//! Lua-surface-syntax tokenizer.
//!
//! Produces `LuaToken` values with source locations. Numbers reuse the
//! patterns from `numeric.rs` (decimal, hex, scientific notation).

use super::scan::{CharCursor, CharIdx};
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

impl LuaTokenLoc {
    /// Bundle a token with its source location and byte length. Reached through
    /// [`LuaLexer::spanned`] for lexemes whose length is measured from the
    /// cursor; called directly for zero/one-width synthetic tokens (Eof) here
    /// and for the parser's Eof sentinel.
    pub(super) fn new(token: LuaToken, loc: SourceLoc, len: usize) -> Self {
        LuaTokenLoc { token, loc, len }
    }
}

pub struct LuaLexer {
    cursor: CharCursor,
    file: String,
}

mod literals;

impl LuaLexer {
    pub fn new(input: &str, file: &str) -> Self {
        LuaLexer {
            cursor: CharCursor::new(input),
            file: file.to_string(),
        }
    }

    fn loc(&self) -> SourceLoc {
        SourceLoc::new(&self.file, self.cursor.line(), self.cursor.col())
    }

    /// Bundle `token` with the span from `start` to the cursor's current
    /// position, deriving `len` via the cursor in one place.
    fn spanned(&self, token: LuaToken, loc: SourceLoc, start: CharIdx) -> LuaTokenLoc {
        LuaTokenLoc::new(token, loc, self.cursor.offset_from(start))
    }

    fn peek(&self) -> Option<char> {
        self.cursor.peek()
    }

    fn peek2(&self) -> Option<char> {
        self.cursor.nth(1)
    }

    fn advance(&mut self) -> Option<char> {
        self.cursor.advance()
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
        let start_line = self.cursor.line();
        loop {
            match self.advance() {
                None => {
                    return Err(format!(
                        "{}:{}:{}: unterminated block comment starting at line {}",
                        self.file,
                        self.cursor.line(),
                        self.cursor.col(),
                        start_line
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

    fn read_ident(&mut self) -> String {
        let start = self.cursor.pos();
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' {
                self.advance();
            } else {
                break;
            }
        }
        self.cursor.span(start).iter().collect()
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
            let start_pos = self.cursor.pos();

            let c = match self.peek() {
                None => {
                    tokens.push(LuaTokenLoc::new(LuaToken::Eof, loc, 0));
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
                        && (self.cursor.nth(1) == Some('[') || self.cursor.nth(1) == Some('='))
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
                        self.file,
                        self.cursor.line(),
                        self.cursor.col(),
                        c
                    ));
                }
            };

            tokens.push(self.spanned(token, loc, start_pos));
        }
    }
}

#[cfg(test)]
mod tests;
