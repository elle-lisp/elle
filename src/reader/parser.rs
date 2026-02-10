use super::token::{OwnedToken, SourceLoc};
use crate::symbol::SymbolTable;
use crate::value::{cons, Value};
use std::rc::Rc;

pub struct Reader {
    tokens: Vec<OwnedToken>,
    locations: Vec<SourceLoc>,
    pos: usize,
}

impl Reader {
    pub fn new(tokens: Vec<OwnedToken>) -> Self {
        // Create default locations for tokens (when not provided with location info)
        let locations = vec![SourceLoc::from_line_col(1, 1); tokens.len()];
        Reader {
            tokens,
            locations,
            pos: 0,
        }
    }

    pub fn with_locations(tokens: Vec<OwnedToken>, locations: Vec<SourceLoc>) -> Self {
        Reader {
            tokens,
            locations,
            pos: 0,
        }
    }

    fn current(&self) -> Option<&OwnedToken> {
        self.tokens.get(self.pos)
    }

    fn current_location(&self) -> SourceLoc {
        self.locations.get(self.pos).cloned().unwrap_or_else(|| {
            // If we're past the end, use the last location
            self.locations
                .last()
                .cloned()
                .unwrap_or_else(SourceLoc::start)
        })
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
                            None => {
                                let loc = self.current_location();
                                return Err(format!(
                                    "{}: unterminated list literal",
                                    loc.position()
                                ));
                            }
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
                    let loc = self.current_location();
                    Err(format!(
                        "{}: @ must be followed by [...] or {{...}}",
                        loc.position()
                    ))
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
            OwnedToken::RightParen => {
                let loc = self.current_location();
                Err(format!(
                    "{}: unexpected closing parenthesis",
                    loc.position()
                ))
            }
            OwnedToken::RightBracket => {
                let loc = self.current_location();
                Err(format!("{}: unexpected closing bracket", loc.position()))
            }
            OwnedToken::RightBrace => {
                let loc = self.current_location();
                Err(format!("{}: unexpected closing brace", loc.position()))
            }
        }
    }

    pub fn read(&mut self, symbols: &mut SymbolTable) -> Result<Value, String> {
        match self.try_read(symbols) {
            Some(result) => result,
            None => {
                let loc = self.current_location();
                Err(format!("{}: unexpected end of input", loc.position()))
            }
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
                None => {
                    let loc = self.current_location();
                    return Err(format!(
                        "{}: unterminated list (missing closing paren)",
                        loc.position()
                    ));
                }
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
                None => {
                    let loc = self.current_location();
                    return Err(format!(
                        "{}: unterminated vector (missing closing bracket)",
                        loc.position()
                    ));
                }
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
                None => {
                    let loc = self.current_location();
                    return Err(format!(
                        "{}: unterminated struct literal (missing closing brace)",
                        loc.position()
                    ));
                }
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
