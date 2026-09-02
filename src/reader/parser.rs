use super::token::{OwnedToken, SourceLoc};
use crate::primitives::ctx::Alloc;
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
    pub fn try_read(
        &mut self,
        ctx: &mut Alloc,
        symbols: &mut SymbolTable,
    ) -> Option<Result<Value, String>> {
        // Skip comment tokens
        while matches!(self.current(), Some(OwnedToken::Comment(_))) {
            self.advance();
        }
        let token = self.current().cloned()?;
        Some(self.read_one(ctx, symbols, &token))
    }

    /// Read a single token/form and return result
    fn read_one(
        &mut self,
        ctx: &mut Alloc,
        symbols: &mut SymbolTable,
        token: &OwnedToken,
    ) -> Result<Value, String> {
        match token {
            // Skip comment tokens — they are handled before reaching here by try_read,
            // but may appear in recursive read() calls inside compound forms.
            OwnedToken::Comment(_) => {
                self.advance();
                self.read(ctx, symbols)
            }
            OwnedToken::LeftParen => self.read_list(ctx, symbols),
            OwnedToken::LeftBracket => self.read_array(ctx, symbols),
            OwnedToken::LeftBrace => self.read_struct(ctx, symbols),
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
                                let list_sym = Value::symbol(symbols.intern("list"));
                                let result = elements
                                    .into_iter()
                                    .rev()
                                    .fold(Value::EMPTY_LIST, |acc, v| ctx.pair(v, acc));
                                return Ok(ctx.pair(list_sym, result));
                            }
                            Some(OwnedToken::Comment(_)) => {
                                self.advance();
                                continue;
                            }
                            _ => elements.push(self.read(ctx, symbols)?),
                        }
                    }
                } else if self.current() == Some(&OwnedToken::LeftBrace) {
                    // Handle @{...} for table sugar
                    self.read_table(ctx, symbols)
                } else if let Some(OwnedToken::String(s)) = self.current().cloned() {
                    // @"..." is sugar for (thaw "...")
                    self.advance();
                    let sb_sym = Value::symbol(symbols.intern("thaw"));
                    let str_val = ctx.string(s.as_str());
                    let inner = ctx.pair(str_val, Value::EMPTY_LIST);
                    Ok(ctx.pair(sb_sym, inner))
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
                let val = self.read(ctx, symbols)?;
                let quote_sym = Value::symbol(symbols.intern("quote"));
                let inner = ctx.pair(val, Value::EMPTY_LIST);
                Ok(ctx.pair(quote_sym, inner))
            }
            OwnedToken::Quasiquote => {
                self.advance();
                let val = self.read(ctx, symbols)?;
                let qq_sym = Value::symbol(symbols.intern("quasiquote"));
                let inner = ctx.pair(val, Value::EMPTY_LIST);
                Ok(ctx.pair(qq_sym, inner))
            }
            OwnedToken::Unquote => {
                self.advance();
                let val = self.read(ctx, symbols)?;
                let uq_sym = Value::symbol(symbols.intern("unquote"));
                let inner = ctx.pair(val, Value::EMPTY_LIST);
                Ok(ctx.pair(uq_sym, inner))
            }
            OwnedToken::UnquoteSplicing => {
                self.advance();
                let val = self.read(ctx, symbols)?;
                let uqs_sym = Value::symbol(symbols.intern("unquote-splicing"));
                let inner = ctx.pair(val, Value::EMPTY_LIST);
                Ok(ctx.pair(uqs_sym, inner))
            }
            OwnedToken::Splice => {
                self.advance();
                let val = self.read(ctx, symbols)?;
                let splice_sym = Value::symbol(symbols.intern("splice"));
                let inner = ctx.pair(val, Value::EMPTY_LIST);
                Ok(ctx.pair(splice_sym, inner))
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
                let val = ctx.string(s.as_str());
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
                let id = symbols.intern(s);
                self.advance();
                Ok(Value::symbol(id))
            }
            OwnedToken::Keyword(s) => {
                // Keywords are self-evaluating; the spelling is learned here —
                // the value-reader is a learning site like the analyzer.
                symbols.keyword(s);
                self.advance();
                Ok(Value::keyword(s))
            }
            OwnedToken::Pipe => {
                let loc = self.current_location();
                Err(format!(
                    "{}: unexpected | (set literals not yet supported)",
                    loc.position()
                ))
            }
            OwnedToken::AtPipe => {
                let loc = self.current_location();
                Err(format!(
                    "{}: unexpected @| (mutable set literals not yet supported)",
                    loc.position()
                ))
            }
            OwnedToken::BytesBracket => {
                let loc = self.current_location();
                Err(format!(
                    "{}: bytes literals not supported in legacy parser",
                    loc.position()
                ))
            }
            OwnedToken::AtBytesBracket => {
                let loc = self.current_location();
                Err(format!(
                    "{}: bytes literals not supported in legacy parser",
                    loc.position()
                ))
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

    pub fn read(&mut self, ctx: &mut Alloc, symbols: &mut SymbolTable) -> Result<Value, String> {
        match self.try_read(ctx, symbols) {
            Some(result) => result,
            None => {
                let loc = self.current_location();
                Err(format!("{}: unexpected end of input", loc.position()))
            }
        }
    }

    fn read_list(&mut self, ctx: &mut Alloc, symbols: &mut SymbolTable) -> Result<Value, String> {
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
                        .fold(Value::EMPTY_LIST, |acc, v| ctx.pair(v, acc)));
                }
                Some(OwnedToken::Comment(_)) => {
                    self.advance();
                    continue;
                }
                _ => elements.push(self.read(ctx, symbols)?),
            }
        }
    }

    fn read_array(&mut self, ctx: &mut Alloc, symbols: &mut SymbolTable) -> Result<Value, String> {
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
                    return Ok(ctx.array_mut(elements));
                }
                Some(OwnedToken::Comment(_)) => {
                    self.advance();
                    continue;
                }
                _ => elements.push(self.read(ctx, symbols)?),
            }
        }
    }

    fn read_struct(&mut self, ctx: &mut Alloc, symbols: &mut SymbolTable) -> Result<Value, String> {
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
                    let struct_sym = Value::symbol(symbols.intern("struct"));
                    let result = elements
                        .into_iter()
                        .rev()
                        .fold(Value::EMPTY_LIST, |acc, v| ctx.pair(v, acc));
                    return Ok(ctx.pair(struct_sym, result));
                }
                Some(OwnedToken::Comment(_)) => {
                    self.advance();
                    continue;
                }
                _ => elements.push(self.read(ctx, symbols)?),
            }
        }
    }

    fn read_table(&mut self, ctx: &mut Alloc, symbols: &mut SymbolTable) -> Result<Value, String> {
        self.advance(); // skip {
        let mut elements = Vec::new();

        loop {
            match self.current() {
                None => return Err("Unterminated table literal".to_string()),
                Some(OwnedToken::RightBrace) => {
                    self.advance();
                    // Build (table k1 v1 k2 v2 ...)
                    let table_sym = Value::symbol(symbols.intern("table"));
                    let result = elements
                        .into_iter()
                        .rev()
                        .fold(Value::EMPTY_LIST, |acc, v| ctx.pair(v, acc));
                    return Ok(ctx.pair(table_sym, result));
                }
                Some(OwnedToken::Comment(_)) => {
                    self.advance();
                    continue;
                }
                _ => elements.push(self.read(ctx, symbols)?),
            }
        }
    }
}
