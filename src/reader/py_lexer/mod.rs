//! Python-surface-syntax tokenizer.
//!
//! Produces `PyToken` values with source locations.  Tracks indentation
//! via an explicit indent stack, emitting synthetic `Indent` and `Dedent`
//! tokens at block boundaries.

use super::scan::{CharCursor, CharIdx};
use super::token::SourceLoc;

/// Token types for the Python surface syntax.
#[derive(Debug, Clone, PartialEq)]
pub enum PyToken {
    // Literals
    Int(i64),
    Float(f64),
    String(String),
    FString(Vec<FStringPart>),
    True,
    False,
    None,

    // Identifiers
    Ident(String),

    // Keywords
    Def,
    Return,
    If,
    Elif,
    Else,
    While,
    For,
    In,
    And,
    Or,
    Not,
    Break,
    Continue,
    Pass,
    Lambda,
    Class,
    Import,
    From,
    As,
    Try,
    Except,
    Finally,
    Raise,
    With,
    Yield,
    Assert,
    Del,
    Global,
    Nonlocal,
    Is,

    // Operators
    Plus,
    Minus,
    Star,
    StarStar, // **
    Slash,
    SlashSlash, // //
    Percent,
    At,  // @ (decorator / matmul)
    Eq,  // ==
    Neq, // !=
    Lt,
    Gt,
    Le,
    Ge,
    Assign,      // =
    PlusAssign,  // +=
    MinusAssign, // -=
    StarAssign,  // *=
    SlashAssign, // /=
    Dot,
    DotDotDot,   // ... (Ellipsis)
    Arrow,       // ->
    Ampersand,   // &
    Pipe,        // |
    Caret,       // ^
    Tilde,       // ~
    ShiftLeft,   // <<
    ShiftRight,  // >>
    ColonAssign, // :=

    // Delimiters
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Comma,
    Colon,
    Semicolon,

    // Indentation
    Newline,
    Indent,
    Dedent,

    Eof,
}

/// Part of an f-string: either literal text or an interpolated expression.
#[derive(Debug, Clone, PartialEq)]
pub enum FStringPart {
    Lit(String),
    Expr(String),
}

/// A token with its source location and byte length.
#[derive(Debug, Clone)]
pub struct PyTokenLoc {
    pub token: PyToken,
    pub loc: SourceLoc,
    pub len: usize,
}

impl PyTokenLoc {
    /// Bundle a token with its source location and byte length. Reached through
    /// [`PyLexer::spanned`] for measured lexemes; called directly for the
    /// zero/one-width synthetic layout tokens (Indent, Dedent, Newline, Eof)
    /// here and for the parser's Eof sentinel.
    pub(super) fn new(token: PyToken, loc: SourceLoc, len: usize) -> Self {
        PyTokenLoc { token, loc, len }
    }
}

pub struct PyLexer {
    cursor: CharCursor,
    file: String,
    /// Stack of indentation levels (in spaces).  Starts with \[0\].
    indent_stack: Vec<usize>,
    /// Nesting depth of brackets/parens/braces (suppresses newlines).
    bracket_depth: u32,
    /// Whether we're at the beginning of a line (need to check indent).
    at_line_start: bool,
}

impl PyLexer {
    pub fn new(input: &str, file: &str) -> Self {
        PyLexer {
            cursor: CharCursor::new(input),
            file: file.to_string(),
            indent_stack: vec![0],
            bracket_depth: 0,
            at_line_start: true,
        }
    }

    fn loc(&self) -> SourceLoc {
        SourceLoc::new(&self.file, self.cursor.line(), self.cursor.col())
    }

    /// Bundle `token` with the span from `start` to the cursor's current
    /// position, deriving `len` via the cursor in one place.
    fn spanned(&self, token: PyToken, loc: SourceLoc, start: CharIdx) -> PyTokenLoc {
        PyTokenLoc::new(token, loc, self.cursor.offset_from(start))
    }

    fn peek(&self) -> Option<char> {
        self.cursor.peek()
    }

    fn peek2(&self) -> Option<char> {
        self.cursor.nth(1)
    }

    fn peek3(&self) -> Option<char> {
        self.cursor.nth(2)
    }

    fn advance(&mut self) -> Option<char> {
        self.cursor.advance()
    }

    /// Returns the number of spaces (tabs count as 4 spaces).
    fn measure_indent(&self) -> usize {
        let mut spaces = 0;
        let mut off = 0;
        while let Some(c) = self.cursor.nth(off) {
            match c {
                ' ' => {
                    spaces += 1;
                    off += 1;
                }
                '\t' => {
                    spaces += 4;
                    off += 1;
                }
                _ => break,
            }
        }
        spaces
    }

    /// Tokenize the entire input, returning all tokens with locations.
    pub fn tokenize(&mut self) -> Result<Vec<PyTokenLoc>, String> {
        let mut tokens = Vec::new();

        loop {
            // Handle indentation at the start of a line
            if self.at_line_start && self.bracket_depth == 0 {
                self.at_line_start = false;

                // Skip blank lines and comment-only lines. Scan ahead by an
                // offset from the cursor, then consume that many chars.
                loop {
                    // Skip spaces/tabs
                    let mut off = 0;
                    while matches!(self.cursor.nth(off), Some(' ') | Some('\t')) {
                        off += 1;
                    }
                    if self.cursor.nth(off) == Some('#') {
                        // Comment-only line: skip to and past the next newline.
                        while let Some(c) = self.cursor.nth(off) {
                            off += 1; // consume this char
                            if c == '\n' {
                                break;
                            }
                        }
                        for _ in 0..off {
                            self.advance();
                        }
                        continue;
                    }
                    if self.cursor.nth(off) == Some('\n') {
                        // Blank line: skip it, newline included.
                        for _ in 0..=off {
                            self.advance();
                        }
                        continue;
                    }
                    break;
                }

                if self.peek().is_none() {
                    // EOF — emit dedents for all remaining indent levels
                    let loc = self.loc();
                    while self.indent_stack.len() > 1 {
                        self.indent_stack.pop();
                        tokens.push(PyTokenLoc::new(PyToken::Dedent, loc.clone(), 0));
                    }
                    tokens.push(PyTokenLoc::new(PyToken::Eof, loc, 0));
                    return Ok(tokens);
                }

                let indent = self.measure_indent();
                let current = *self.indent_stack.last().unwrap();

                if indent > current {
                    self.indent_stack.push(indent);
                    tokens.push(PyTokenLoc::new(PyToken::Indent, self.loc(), 0));
                } else if indent < current {
                    while *self.indent_stack.last().unwrap() > indent {
                        self.indent_stack.pop();
                        tokens.push(PyTokenLoc::new(PyToken::Dedent, self.loc(), 0));
                    }
                    if *self.indent_stack.last().unwrap() != indent {
                        return Err(format!(
                            "{}:{}:{}: inconsistent dedent",
                            self.file,
                            self.cursor.line(),
                            self.cursor.col()
                        ));
                    }
                }

                // Skip the whitespace we just measured
                while self.peek().is_some_and(|c| c == ' ' || c == '\t') {
                    self.advance();
                }
                continue;
            }

            // Skip spaces (not newlines, not tabs at line start)
            while self.peek().is_some_and(|c| c == ' ' || c == '\t') {
                self.advance();
            }

            let loc = self.loc();
            let start_pos = self.cursor.pos();

            let c = match self.peek() {
                None => {
                    // EOF — emit remaining dedents
                    while self.indent_stack.len() > 1 {
                        self.indent_stack.pop();
                        tokens.push(PyTokenLoc::new(PyToken::Dedent, loc.clone(), 0));
                    }
                    tokens.push(PyTokenLoc::new(PyToken::Eof, loc, 0));
                    return Ok(tokens);
                }
                Some(c) => c,
            };

            // Newline
            if c == '\n' {
                self.advance();
                if self.bracket_depth == 0 {
                    // Only emit Newline if previous token wasn't already Newline
                    let should_emit = tokens
                        .last()
                        .map(|t| !matches!(t.token, PyToken::Newline | PyToken::Indent))
                        .unwrap_or(false);
                    if should_emit {
                        tokens.push(PyTokenLoc::new(PyToken::Newline, loc.clone(), 1));
                    }
                    self.at_line_start = true;
                }
                continue;
            }

            // Line continuation
            if c == '\\' && self.peek2() == Some('\n') {
                self.advance(); // backslash
                self.advance(); // newline
                continue;
            }

            let token = match c {
                // Comments
                '#' => {
                    while self.peek().is_some_and(|c| c != '\n') {
                        self.advance();
                    }
                    continue;
                }

                // String literals (including f-strings)
                'f' | 'F' if matches!(self.peek2(), Some('"') | Some('\'')) => {
                    self.advance(); // skip 'f'
                    let quote = self.peek().unwrap();
                    self.advance(); // skip opening quote
                    let triple = self.peek() == Some(quote) && self.peek2() == Some(quote);
                    let parts = self.read_fstring(quote, triple)?;
                    PyToken::FString(parts)
                }
                'r' | 'R' if matches!(self.peek2(), Some('"') | Some('\'')) => {
                    self.advance(); // skip 'r'
                    let quote = self.peek().unwrap();
                    self.advance(); // skip opening quote
                                    // Raw string: no escape processing
                    let triple = self.peek() == Some(quote) && self.peek2() == Some(quote);
                    if triple {
                        self.advance();
                        self.advance();
                    }
                    let mut s = String::new();
                    loop {
                        match self.advance() {
                            None => {
                                return Err(format!("{}: unterminated raw string", loc.position()));
                            }
                            Some(c) if c == quote => {
                                if triple {
                                    if self.peek() == Some(quote) && self.peek2() == Some(quote) {
                                        self.advance();
                                        self.advance();
                                        break;
                                    }
                                    s.push(c);
                                } else {
                                    break;
                                }
                            }
                            Some(c) => s.push(c),
                        }
                    }
                    PyToken::String(s)
                }
                'b' | 'B' if matches!(self.peek2(), Some('"') | Some('\'')) => {
                    // Byte strings — treat as regular strings for now
                    self.advance();
                    let quote = self.peek().unwrap();
                    self.advance();
                    let triple = self.peek() == Some(quote) && self.peek2() == Some(quote);
                    let s = self.read_string(quote, triple)?;
                    PyToken::String(s)
                }
                '"' | '\'' => {
                    self.advance(); // skip opening quote
                    let triple = self.peek() == Some(c) && self.peek2() == Some(c);
                    let s = self.read_string(c, triple)?;
                    PyToken::String(s)
                }

                // Numbers
                '0'..='9' => self.read_number()?,

                // Identifiers and keywords
                c if c.is_alphabetic() || c == '_' => {
                    let name = self.read_ident();
                    self.keyword_or_ident(&name)
                }

                // Three-char operators
                '.' if self.peek2() == Some('.') && self.peek3() == Some('.') => {
                    self.advance();
                    self.advance();
                    self.advance();
                    PyToken::DotDotDot
                }
                '*' if self.peek2() == Some('*') => {
                    self.advance();
                    self.advance();
                    PyToken::StarStar
                }
                '/' if self.peek2() == Some('/') => {
                    self.advance();
                    self.advance();
                    PyToken::SlashSlash
                }

                // Two-char operators
                '=' if self.peek2() == Some('=') => {
                    self.advance();
                    self.advance();
                    PyToken::Eq
                }
                '!' if self.peek2() == Some('=') => {
                    self.advance();
                    self.advance();
                    PyToken::Neq
                }
                '<' if self.peek2() == Some('=') => {
                    self.advance();
                    self.advance();
                    PyToken::Le
                }
                '>' if self.peek2() == Some('=') => {
                    self.advance();
                    self.advance();
                    PyToken::Ge
                }
                '<' if self.peek2() == Some('<') => {
                    self.advance();
                    self.advance();
                    PyToken::ShiftLeft
                }
                '>' if self.peek2() == Some('>') => {
                    self.advance();
                    self.advance();
                    PyToken::ShiftRight
                }
                '+' if self.peek2() == Some('=') => {
                    self.advance();
                    self.advance();
                    PyToken::PlusAssign
                }
                '-' if self.peek2() == Some('=') => {
                    self.advance();
                    self.advance();
                    PyToken::MinusAssign
                }
                '*' if self.peek2() == Some('=') => {
                    self.advance();
                    self.advance();
                    PyToken::StarAssign
                }
                '/' if self.peek2() == Some('=') => {
                    self.advance();
                    self.advance();
                    PyToken::SlashAssign
                }
                '-' if self.peek2() == Some('>') => {
                    self.advance();
                    self.advance();
                    PyToken::Arrow
                }
                ':' if self.peek2() == Some('=') => {
                    self.advance();
                    self.advance();
                    PyToken::ColonAssign
                }

                // Single-char operators and delimiters
                '+' => {
                    self.advance();
                    PyToken::Plus
                }
                '-' => {
                    self.advance();
                    PyToken::Minus
                }
                '*' => {
                    self.advance();
                    PyToken::Star
                }
                '/' => {
                    self.advance();
                    PyToken::Slash
                }
                '%' => {
                    self.advance();
                    PyToken::Percent
                }
                '@' => {
                    self.advance();
                    PyToken::At
                }
                '<' => {
                    self.advance();
                    PyToken::Lt
                }
                '>' => {
                    self.advance();
                    PyToken::Gt
                }
                '=' => {
                    self.advance();
                    PyToken::Assign
                }
                '.' => {
                    self.advance();
                    PyToken::Dot
                }
                '&' => {
                    self.advance();
                    PyToken::Ampersand
                }
                '|' => {
                    self.advance();
                    PyToken::Pipe
                }
                '^' => {
                    self.advance();
                    PyToken::Caret
                }
                '~' => {
                    self.advance();
                    PyToken::Tilde
                }
                ':' => {
                    self.advance();
                    PyToken::Colon
                }
                ';' => {
                    self.advance();
                    PyToken::Semicolon
                }
                ',' => {
                    self.advance();
                    PyToken::Comma
                }
                '(' => {
                    self.advance();
                    self.bracket_depth += 1;
                    PyToken::LParen
                }
                ')' => {
                    self.advance();
                    self.bracket_depth = self.bracket_depth.saturating_sub(1);
                    PyToken::RParen
                }
                '[' => {
                    self.advance();
                    self.bracket_depth += 1;
                    PyToken::LBracket
                }
                ']' => {
                    self.advance();
                    self.bracket_depth = self.bracket_depth.saturating_sub(1);
                    PyToken::RBracket
                }
                '{' => {
                    self.advance();
                    self.bracket_depth += 1;
                    PyToken::LBrace
                }
                '}' => {
                    self.advance();
                    self.bracket_depth = self.bracket_depth.saturating_sub(1);
                    PyToken::RBrace
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

mod literals;
#[cfg(test)]
mod tests;
