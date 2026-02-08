use crate::symbol::SymbolTable;
use crate::value::{cons, Value};
use std::rc::Rc;

/// Fast delimiter check - O(1) instead of string contains O(n)
/// Checks if a character is a Lisp delimiter
#[inline]
fn is_delimiter(c: char) -> bool {
    matches!(
        c,
        '(' | ')' | '[' | ']' | '{' | '}' | '\'' | '`' | ',' | ':' | '@'
    )
}

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

pub struct Lexer<'a> {
    input: &'a str,
    bytes: &'a [u8],
    pos: usize,
    line: usize,
    col: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Lexer {
            input,
            bytes: input.as_bytes(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    fn get_loc(&self) -> SourceLoc {
        SourceLoc::new(self.line, self.col)
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

    /// Read a symbol and return a slice of the original input
    fn read_symbol(&mut self) -> (usize, usize) {
        let start = self.pos;
        while let Some(c) = self.current() {
            // Use fast delimiter check instead of string contains()
            if c.is_whitespace() || is_delimiter(c) {
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

pub struct Reader {
    tokens: Vec<OwnedToken>,
    pos: usize,
}

impl Reader {
    pub fn new(tokens: Vec<OwnedToken>) -> Self {
        Reader { tokens, pos: 0 }
    }

    fn current(&self) -> Option<&OwnedToken> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<OwnedToken> {
        let token = self.current().cloned();
        self.pos += 1;
        token
    }

    /// Try to read a single value from the token stream.
    /// Returns None if at EOF (not an error), Some(Err(_)) if there's a parse error.
    pub fn try_read(&mut self, symbols: &mut SymbolTable) -> Option<Result<Value, String>> {
        let token = self.current().cloned()?;
        Some(self.read_one(symbols, &token))
    }

    /// Read a single token/form and return result
    fn read_one(&mut self, symbols: &mut SymbolTable, token: &OwnedToken) -> Result<Value, String> {
        match token {
            OwnedToken::LeftParen => self.read_list(symbols),
            OwnedToken::LeftBracket => self.read_vector(symbols),
            OwnedToken::LeftBrace => self.read_struct(symbols),
            OwnedToken::ListSugar => {
                self.advance();
                // @[...] is sugar for (list ...)
                // @{...} is sugar for (table ...)
                if self.current() == Some(&OwnedToken::LeftBracket) {
                    self.advance(); // skip [
                    let mut elements = Vec::new();

                    loop {
                        match self.current() {
                            None => return Err("Unterminated list literal".to_string()),
                            Some(OwnedToken::RightBracket) => {
                                self.advance();
                                // Build (list e1 e2 e3 ...)
                                let list_sym = Value::Symbol(symbols.intern("list"));
                                let result = elements
                                    .into_iter()
                                    .rev()
                                    .fold(Value::Nil, |acc, v| cons(v, acc));
                                return Ok(cons(list_sym, result));
                            }
                            _ => elements.push(self.read(symbols)?),
                        }
                    }
                } else if self.current() == Some(&OwnedToken::LeftBrace) {
                    // Handle @{...} for table sugar
                    self.read_table(symbols)
                } else {
                    Err("@ must be followed by [...] or {...}".to_string())
                }
            }

            OwnedToken::Quote => {
                self.advance();
                let val = self.read(symbols)?;
                let quote_sym = Value::Symbol(symbols.intern("quote"));
                Ok(cons(quote_sym, cons(val, Value::Nil)))
            }
            OwnedToken::Quasiquote => {
                self.advance();
                let val = self.read(symbols)?;
                let qq_sym = Value::Symbol(symbols.intern("quasiquote"));
                Ok(cons(qq_sym, cons(val, Value::Nil)))
            }
            OwnedToken::Unquote => {
                self.advance();
                let val = self.read(symbols)?;
                let uq_sym = Value::Symbol(symbols.intern("unquote"));
                Ok(cons(uq_sym, cons(val, Value::Nil)))
            }
            OwnedToken::UnquoteSplicing => {
                self.advance();
                let val = self.read(symbols)?;
                let uqs_sym = Value::Symbol(symbols.intern("unquote-splicing"));
                Ok(cons(uqs_sym, cons(val, Value::Nil)))
            }
            OwnedToken::Integer(n) => {
                let val = Value::Int(*n);
                self.advance();
                Ok(val)
            }
            OwnedToken::Float(f) => {
                let val = Value::Float(*f);
                self.advance();
                Ok(val)
            }
            OwnedToken::String(s) => {
                let val = Value::String(Rc::from(s.as_str()));
                self.advance();
                Ok(val)
            }
            OwnedToken::Bool(b) => {
                let val = Value::Bool(*b);
                self.advance();
                Ok(val)
            }
            OwnedToken::Nil => {
                self.advance();
                Ok(Value::Nil)
            }
            OwnedToken::Symbol(s) => {
                // Check if this is a qualified symbol (e.g., "list:length")
                let (module_name, symbol_name) = Self::parse_qualified_symbol(s);
                if !symbol_name.is_empty() {
                    // This is a qualified symbol - represent as: (qualified-ref module-name symbol-name)
                    let module_sym = symbols.intern(&module_name);
                    let name_sym = symbols.intern(&symbol_name);
                    let qualified_ref = symbols.intern("qualified-ref");
                    // Build list: (qualified-ref module symbol)
                    let result = cons(
                        Value::Symbol(qualified_ref),
                        cons(
                            Value::Symbol(module_sym),
                            cons(Value::Symbol(name_sym), Value::Nil),
                        ),
                    );
                    self.advance();
                    Ok(result)
                } else {
                    // Regular unqualified symbol
                    let id = symbols.intern(s);
                    self.advance();
                    Ok(Value::Symbol(id))
                }
            }
            OwnedToken::Keyword(s) => {
                // Keywords are self-evaluating values
                let id = symbols.intern(s);
                self.advance();
                Ok(Value::Keyword(id))
            }
            OwnedToken::RightParen => Err("Unexpected )".to_string()),
            OwnedToken::RightBracket => Err("Unexpected ]".to_string()),
            OwnedToken::RightBrace => Err("Unexpected }".to_string()),
        }
    }

    pub fn read(&mut self, symbols: &mut SymbolTable) -> Result<Value, String> {
        match self.try_read(symbols) {
            Some(result) => result,
            None => Err("Unexpected EOF".to_string()), // Keep old API for backward compat
        }
    }

    /// Parse a module-qualified symbol (e.g., "module:symbol")
    /// Returns (module_slice, symbol_slice) if qualified, or (symbol_slice, "") if unqualified
    fn parse_qualified_symbol(sym: &str) -> (String, String) {
        if let Some(colon_pos) = sym.rfind(':') {
            let module = sym[..colon_pos].to_string();
            let name = sym[colon_pos + 1..].to_string();
            if !module.is_empty() && !name.is_empty() {
                return (module, name);
            }
        }
        (sym.to_string(), String::new())
    }

    fn read_list(&mut self, symbols: &mut SymbolTable) -> Result<Value, String> {
        self.advance(); // skip (
        let mut elements = Vec::new();

        loop {
            match self.current() {
                None => return Err("Unterminated list".to_string()),
                Some(OwnedToken::RightParen) => {
                    self.advance();
                    return Ok(elements
                        .into_iter()
                        .rev()
                        .fold(Value::Nil, |acc, v| cons(v, acc)));
                }
                _ => elements.push(self.read(symbols)?),
            }
        }
    }

    fn read_vector(&mut self, symbols: &mut SymbolTable) -> Result<Value, String> {
        self.advance(); // skip [
        let mut elements = Vec::new();

        loop {
            match self.current() {
                None => return Err("Unterminated vector".to_string()),
                Some(OwnedToken::RightBracket) => {
                    self.advance();
                    return Ok(Value::Vector(Rc::new(elements)));
                }
                _ => elements.push(self.read(symbols)?),
            }
        }
    }

    fn read_struct(&mut self, symbols: &mut SymbolTable) -> Result<Value, String> {
        self.advance(); // skip {
        let mut elements = Vec::new();

        loop {
            match self.current() {
                None => return Err("Unterminated struct literal".to_string()),
                Some(OwnedToken::RightBrace) => {
                    self.advance();
                    // Build (struct k1 v1 k2 v2 ...)
                    let struct_sym = Value::Symbol(symbols.intern("struct"));
                    let result = elements
                        .into_iter()
                        .rev()
                        .fold(Value::Nil, |acc, v| cons(v, acc));
                    return Ok(cons(struct_sym, result));
                }
                _ => elements.push(self.read(symbols)?),
            }
        }
    }

    fn read_table(&mut self, symbols: &mut SymbolTable) -> Result<Value, String> {
        self.advance(); // skip {
        let mut elements = Vec::new();

        loop {
            match self.current() {
                None => return Err("Unterminated table literal".to_string()),
                Some(OwnedToken::RightBrace) => {
                    self.advance();
                    // Build (table k1 v1 k2 v2 ...)
                    let table_sym = Value::Symbol(symbols.intern("table"));
                    let result = elements
                        .into_iter()
                        .rev()
                        .fold(Value::Nil, |acc, v| cons(v, acc));
                    return Ok(cons(table_sym, result));
                }
                _ => elements.push(self.read(symbols)?),
            }
        }
    }
}

pub fn read_str(input: &str, symbols: &mut SymbolTable) -> Result<Value, String> {
    // Strip shebang if present (e.g., #!/usr/bin/env elle)
    let input_owned = if input.starts_with("#!") {
        // Find the end of the first line and skip it
        input.lines().skip(1).collect::<Vec<_>>().join("\n")
    } else {
        input.to_string()
    };

    let mut lexer = Lexer::new(&input_owned);
    let mut tokens = Vec::new();

    while let Some(token) = lexer.next_token()? {
        tokens.push(OwnedToken::from(token));
    }

    if tokens.is_empty() {
        return Err("No input".to_string());
    }

    let mut reader = Reader::new(tokens);
    reader.read(symbols)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::approx_constant)]
    fn test_read_number() {
        let mut symbols = SymbolTable::new();
        assert_eq!(read_str("42", &mut symbols).unwrap(), Value::Int(42));
        assert_eq!(read_str("3.14", &mut symbols).unwrap(), Value::Float(3.14));
    }

    #[test]
    fn test_read_list() {
        let mut symbols = SymbolTable::new();
        let result = read_str("(1 2 3)", &mut symbols).unwrap();
        assert!(result.is_list());
        let vec = result.list_to_vec().unwrap();
        assert_eq!(vec.len(), 3);
    }

    #[test]
    fn test_read_quote() {
        let mut symbols = SymbolTable::new();
        let result = read_str("'foo", &mut symbols).unwrap();
        let vec = result.list_to_vec().unwrap();
        assert_eq!(vec.len(), 2);
    }
}
