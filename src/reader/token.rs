use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLoc {
    pub file: String,
    pub line: usize,
    pub col: usize,
}

impl fmt::Display for SourceLoc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.file, self.line, self.col)
    }
}

impl SourceLoc {
    pub fn new(file: impl Into<String>, line: usize, col: usize) -> Self {
        SourceLoc {
            file: file.into(),
            line,
            col,
        }
    }

    /// Create a location from line and column (file set to unknown)
    pub fn from_line_col(line: usize, col: usize) -> Self {
        SourceLoc {
            file: "<unknown>".to_string(),
            line,
            col,
        }
    }

    /// Create a location at the beginning of a file
    pub fn start() -> Self {
        SourceLoc {
            file: "<unknown>".to_string(),
            line: 1,
            col: 1,
        }
    }

    /// Get position as "file:line:col" string
    pub fn position(&self) -> String {
        format!("{}:{}:{}", self.file, self.line, self.col)
    }

    /// Check if this is an unknown location
    pub fn is_unknown(&self) -> bool {
        self.file == "<unknown>"
    }

    /// Create a copy with a different file name
    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.file = file.into();
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TokenWithLoc<'a> {
    pub token: Token<'a>,
    pub loc: SourceLoc,
    /// Source byte length of the token. Computed by the lexer so the syntax
    /// parser can build accurate spans without per-token-type heuristics.
    /// Previously, span width was hardcoded per variant (and wrong for
    /// multi-digit integers and floats) or bolted onto individual variants
    /// like `Bool(bool, usize)`.
    pub len: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Token<'a> {
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    LeftBrace,
    RightBrace,
    Quote,
    Quasiquote,
    Unquote,
    UnquoteSplicing,
    ListSugar, // @ for list sugar
    Symbol(&'a str),
    Keyword(&'a str),
    Integer(i64),
    Float(f64),
    String(String),
    Bool(bool),
    Nil,
}

/// Owned token variant for storage in Reader
#[derive(Debug, Clone, PartialEq)]
pub enum OwnedToken {
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    LeftBrace,
    RightBrace,
    Quote,
    Quasiquote,
    Unquote,
    UnquoteSplicing,
    ListSugar,
    Symbol(String),
    Keyword(String),
    Integer(i64),
    Float(f64),
    String(String),
    Bool(bool),
    Nil,
}

impl<'a> From<Token<'a>> for OwnedToken {
    fn from(token: Token<'a>) -> Self {
        match token {
            Token::LeftParen => OwnedToken::LeftParen,
            Token::RightParen => OwnedToken::RightParen,
            Token::LeftBracket => OwnedToken::LeftBracket,
            Token::RightBracket => OwnedToken::RightBracket,
            Token::LeftBrace => OwnedToken::LeftBrace,
            Token::RightBrace => OwnedToken::RightBrace,
            Token::Quote => OwnedToken::Quote,
            Token::Quasiquote => OwnedToken::Quasiquote,
            Token::Unquote => OwnedToken::Unquote,
            Token::UnquoteSplicing => OwnedToken::UnquoteSplicing,
            Token::ListSugar => OwnedToken::ListSugar,
            Token::Symbol(s) => OwnedToken::Symbol(s.to_string()),
            Token::Keyword(s) => OwnedToken::Keyword(s.to_string()),
            Token::Integer(i) => OwnedToken::Integer(i),
            Token::Float(f) => OwnedToken::Float(f),
            Token::String(s) => OwnedToken::String(s),
            Token::Bool(b) => OwnedToken::Bool(b),
            Token::Nil => OwnedToken::Nil,
        }
    }
}
