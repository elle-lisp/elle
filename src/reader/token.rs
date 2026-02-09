#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceLoc {
    pub line: usize,
    pub col: usize,
}

impl SourceLoc {
    pub fn new(line: usize, col: usize) -> Self {
        SourceLoc { line, col }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TokenWithLoc<'a> {
    pub token: Token<'a>,
    pub loc: SourceLoc,
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
