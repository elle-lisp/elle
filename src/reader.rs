use crate::symbol::SymbolTable;
use crate::value::{cons, Value};
use std::rc::Rc;

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
pub struct TokenWithLoc {
    pub token: Token,
    pub loc: SourceLoc,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    Quote,
    Quasiquote,
    Unquote,
    UnquoteSplicing,
    ListSugar, // @ for list sugar
    Symbol(String),
    Keyword(String),
    Integer(i64),
    Float(f64),
    String(String),
    Bool(bool),
    Nil,
}

pub struct Lexer {
    input: Vec<char>,
    pos: usize,
    line: usize,
    col: usize,
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Lexer {
            input: input.chars().collect(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    fn get_loc(&self) -> SourceLoc {
        SourceLoc::new(self.line, self.col)
    }

    fn current(&self) -> Option<char> {
        self.input.get(self.pos).copied()
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
        }
        self.pos += 1;
        c
    }

    fn peek(&self, offset: usize) -> Option<char> {
        self.input.get(self.pos + offset).copied()
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

    fn read_number(&mut self) -> Result<Token, String> {
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

    fn read_symbol(&mut self) -> String {
        let mut sym = String::new();
        while let Some(c) = self.current() {
            if c.is_whitespace() || "()[]'`,:@".contains(c) {
                break;
            }
            sym.push(c);
            self.advance();
        }
        sym
    }

    /// Read a module-qualified symbol (e.g., "module:symbol")
    /// Returns (module_name, symbol_name) if qualified, or (symbol_name, "") if unqualified
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

    pub fn next_token_with_loc(&mut self) -> Result<Option<TokenWithLoc>, String> {
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
                let keyword = self.read_symbol();
                if keyword.is_empty() {
                    Err("Invalid keyword: expected symbol after :".to_string())
                } else {
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
                        Ok(Some(TokenWithLoc {
                            token: Token::Symbol(self.read_symbol()),
                            loc,
                        }))
                    } else {
                        self.read_number()
                            .map(|t| Some(TokenWithLoc { token: t, loc }))
                    }
                } else if c == '-' || c == '+' {
                    Ok(Some(TokenWithLoc {
                        token: Token::Symbol(self.read_symbol()),
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
                let sym = self.read_symbol();
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

    pub fn next_token(&mut self) -> Result<Option<Token>, String> {
        self.next_token_with_loc()
            .map(|opt| opt.map(|twl| twl.token))
    }
}

pub struct Reader {
    tokens: Vec<Token>,
    pos: usize,
}

impl Reader {
    pub fn new(tokens: Vec<Token>) -> Self {
        Reader { tokens, pos: 0 }
    }

    fn current(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<Token> {
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
    fn read_one(&mut self, symbols: &mut SymbolTable, token: &Token) -> Result<Value, String> {
        match token {
            Token::LeftParen => self.read_list(symbols),
            Token::LeftBracket => self.read_vector(symbols),
            Token::ListSugar => {
                self.advance();
                // @[...] is sugar for (list ...)
                if self.current() == Some(&Token::LeftBracket) {
                    self.advance(); // skip [
                    let mut elements = Vec::new();

                    loop {
                        match self.current() {
                            None => return Err("Unterminated list literal".to_string()),
                            Some(Token::RightBracket) => {
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
                } else {
                    Err("@ must be followed by [...]".to_string())
                }
            }
            Token::Quote => {
                self.advance();
                let val = self.read(symbols)?;
                let quote_sym = Value::Symbol(symbols.intern("quote"));
                Ok(cons(quote_sym, cons(val, Value::Nil)))
            }
            Token::Quasiquote => {
                self.advance();
                let val = self.read(symbols)?;
                let qq_sym = Value::Symbol(symbols.intern("quasiquote"));
                Ok(cons(qq_sym, cons(val, Value::Nil)))
            }
            Token::Unquote => {
                self.advance();
                let val = self.read(symbols)?;
                let uq_sym = Value::Symbol(symbols.intern("unquote"));
                Ok(cons(uq_sym, cons(val, Value::Nil)))
            }
            Token::UnquoteSplicing => {
                self.advance();
                let val = self.read(symbols)?;
                let uqs_sym = Value::Symbol(symbols.intern("unquote-splicing"));
                Ok(cons(uqs_sym, cons(val, Value::Nil)))
            }
            Token::Integer(n) => {
                let val = Value::Int(*n);
                self.advance();
                Ok(val)
            }
            Token::Float(f) => {
                let val = Value::Float(*f);
                self.advance();
                Ok(val)
            }
            Token::String(s) => {
                let val = Value::String(Rc::from(s.as_str()));
                self.advance();
                Ok(val)
            }
            Token::Bool(b) => {
                let val = Value::Bool(*b);
                self.advance();
                Ok(val)
            }
            Token::Nil => {
                self.advance();
                Ok(Value::Nil)
            }
            Token::Symbol(s) => {
                // Check if this is a qualified symbol (e.g., "list:length")
                let (module_name, symbol_name) = Lexer::parse_qualified_symbol(s);
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
            Token::Keyword(s) => {
                // Keywords are self-evaluating values
                let id = symbols.intern(s);
                self.advance();
                Ok(Value::Keyword(id))
            }
            Token::RightParen => Err("Unexpected )".to_string()),
            Token::RightBracket => Err("Unexpected ]".to_string()),
        }
    }

    pub fn read(&mut self, symbols: &mut SymbolTable) -> Result<Value, String> {
        match self.try_read(symbols) {
            Some(result) => result,
            None => Err("Unexpected EOF".to_string()), // Keep old API for backward compat
        }
    }

    fn read_list(&mut self, symbols: &mut SymbolTable) -> Result<Value, String> {
        self.advance(); // skip (
        let mut elements = Vec::new();

        loop {
            match self.current() {
                None => return Err("Unterminated list".to_string()),
                Some(Token::RightParen) => {
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
                Some(Token::RightBracket) => {
                    self.advance();
                    return Ok(Value::Vector(Rc::new(elements)));
                }
                _ => elements.push(self.read(symbols)?),
            }
        }
    }
}

pub fn read_str(input: &str, symbols: &mut SymbolTable) -> Result<Value, String> {
    // Strip shebang if present (e.g., #!/usr/bin/env elle)
    let input = if input.starts_with("#!") {
        // Find the end of the first line and skip it
        input.lines().skip(1).collect::<Vec<_>>().join("\n")
    } else {
        input.to_string()
    };

    let mut lexer = Lexer::new(&input);
    let mut tokens = Vec::new();

    while let Some(token) = lexer.next_token()? {
        tokens.push(token);
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
