use super::token::{OwnedToken, SourceLoc};
use crate::symbol::SymbolTable;
use crate::value::repr::Value;

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

    /// Get the current source location (public API)
    pub fn get_current_location(&self) -> SourceLoc {
        self.current_location()
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
            OwnedToken::LeftBracket => self.read_array(symbols),
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
                                let list_sym = Value::symbol(symbols.intern("list").0);
                                let result = elements
                                    .into_iter()
                                    .rev()
                                    .fold(Value::EMPTY_LIST, |acc, v| Value::cons(v, acc));
                                return Ok(Value::cons(list_sym, result));
                            }
                            _ => elements.push(self.read(symbols)?),
                        }
                    }
                } else if self.current() == Some(&OwnedToken::LeftBrace) {
                    // Handle @{...} for table sugar
                    self.read_table(symbols)
                } else if let Some(OwnedToken::String(s)) = self.current().cloned() {
                    // @"..." is sugar for (string->buffer "...")
                    self.advance();
                    let sb_sym = Value::symbol(symbols.intern("string->buffer").0);
                    let str_val = Value::string(s.as_str());
                    Ok(Value::cons(sb_sym, Value::cons(str_val, Value::EMPTY_LIST)))
                } else {
                    let loc = self.current_location();
                    Err(format!(
                        "{}: @ must be followed by [...], {{...}}, or \"...\"",
                        loc.position()
                    ))
                }
            }

            OwnedToken::Quote => {
                self.advance();
                let val = self.read(symbols)?;
                let quote_sym = Value::symbol(symbols.intern("quote").0);
                Ok(Value::cons(quote_sym, Value::cons(val, Value::EMPTY_LIST)))
            }
            OwnedToken::Quasiquote => {
                self.advance();
                let val = self.read(symbols)?;
                let qq_sym = Value::symbol(symbols.intern("quasiquote").0);
                Ok(Value::cons(qq_sym, Value::cons(val, Value::EMPTY_LIST)))
            }
            OwnedToken::Unquote => {
                self.advance();
                let val = self.read(symbols)?;
                let uq_sym = Value::symbol(symbols.intern("unquote").0);
                Ok(Value::cons(uq_sym, Value::cons(val, Value::EMPTY_LIST)))
            }
            OwnedToken::UnquoteSplicing => {
                self.advance();
                let val = self.read(symbols)?;
                let uqs_sym = Value::symbol(symbols.intern("unquote-splicing").0);
                Ok(Value::cons(uqs_sym, Value::cons(val, Value::EMPTY_LIST)))
            }
            OwnedToken::Splice => {
                self.advance();
                let val = self.read(symbols)?;
                let splice_sym = Value::symbol(symbols.intern("splice").0);
                Ok(Value::cons(splice_sym, Value::cons(val, Value::EMPTY_LIST)))
            }
            OwnedToken::Integer(n) => {
                let val = Value::int(*n);
                self.advance();
                Ok(val)
            }
            OwnedToken::Float(f) => {
                let val = Value::float(*f);
                self.advance();
                Ok(val)
            }
            OwnedToken::String(s) => {
                let val = Value::string(s.as_str());
                self.advance();
                Ok(val)
            }
            OwnedToken::Bool(b) => {
                let val = Value::bool(*b);
                self.advance();
                Ok(val)
            }
            OwnedToken::Nil => {
                self.advance();
                Ok(Value::NIL)
            }
            OwnedToken::Symbol(s) => {
                // Check if this is a qualified symbol (e.g., "list:length")
                let (module_name, symbol_name) = Self::parse_qualified_symbol(s);
                if !symbol_name.is_empty() {
                    // This is a qualified symbol - represent as: (qualified-ref module-name symbol-name)
                    let module_sym = symbols.intern(&module_name).0;
                    let name_sym = symbols.intern(&symbol_name).0;
                    let qualified_ref = symbols.intern("qualified-ref").0;
                    // Build list: (qualified-ref module symbol)
                    let result = Value::cons(
                        Value::symbol(qualified_ref),
                        Value::cons(
                            Value::symbol(module_sym),
                            Value::cons(Value::symbol(name_sym), Value::NIL),
                        ),
                    );
                    self.advance();
                    Ok(result)
                } else {
                    // Regular unqualified symbol
                    let id = symbols.intern(s).0;
                    self.advance();
                    Ok(Value::symbol(id))
                }
            }
            OwnedToken::Keyword(s) => {
                // Keywords are self-evaluating values (interned strings)
                self.advance();
                Ok(Value::keyword(s))
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
                        .fold(Value::EMPTY_LIST, |acc, v| Value::cons(v, acc)));
                }
                _ => elements.push(self.read(symbols)?),
            }
        }
    }

    fn read_array(&mut self, symbols: &mut SymbolTable) -> Result<Value, String> {
        self.advance(); // skip [
        let mut elements = Vec::new();

        loop {
            match self.current() {
                None => {
                    let loc = self.current_location();
                    return Err(format!(
                        "{}: unterminated array (missing closing bracket)",
                        loc.position()
                    ));
                }
                Some(OwnedToken::RightBracket) => {
                    self.advance();
                    return Ok(Value::array(elements));
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
                    let struct_sym = Value::symbol(symbols.intern("struct").0);
                    let result = elements
                        .into_iter()
                        .rev()
                        .fold(Value::EMPTY_LIST, |acc, v| Value::cons(v, acc));
                    return Ok(Value::cons(struct_sym, result));
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
                    let table_sym = Value::symbol(symbols.intern("table").0);
                    let result = elements
                        .into_iter()
                        .rev()
                        .fold(Value::EMPTY_LIST, |acc, v| Value::cons(v, acc));
                    return Ok(Value::cons(table_sym, result));
                }
                _ => elements.push(self.read(symbols)?),
            }
        }
    }
}
