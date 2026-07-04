use super::*;

impl PyLexer {
    pub(super) fn read_string(&mut self, quote: char, triple: bool) -> Result<String, String> {
        let start_loc = self.loc();
        if triple {
            // Skip opening triple quote (already consumed first char)
            self.advance();
            self.advance();
        }
        let mut s = String::new();
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
                    Some('\\') => s.push('\\'),
                    Some('\'') => s.push('\''),
                    Some('"') => s.push('"'),
                    Some('0') => s.push('\0'),
                    Some('a') => s.push('\x07'),
                    Some('b') => s.push('\x08'),
                    Some('f') => s.push('\x0C'),
                    Some('v') => s.push('\x0B'),
                    Some('x') => {
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
                    Some('u') => {
                        let mut hex = String::new();
                        for _ in 0..4 {
                            match self.advance() {
                                Some(c) if c.is_ascii_hexdigit() => hex.push(c),
                                _ => {
                                    return Err(format!(
                                        "{}: invalid \\u escape",
                                        start_loc.position()
                                    ))
                                }
                            }
                        }
                        let val = u32::from_str_radix(&hex, 16).unwrap();
                        s.push(char::from_u32(val).unwrap_or('\u{FFFD}'));
                    }
                    Some('\n') => {} // line continuation
                    Some(c) => {
                        s.push('\\');
                        s.push(c);
                    }
                },
                Some(c) if c == quote => {
                    if triple {
                        if self.peek() == Some(quote) && self.peek2() == Some(quote) {
                            self.advance();
                            self.advance();
                            return Ok(s);
                        }
                        s.push(c);
                    } else {
                        return Ok(s);
                    }
                }
                Some(c) => s.push(c),
            }
        }
    }

    /// Read an f-string, collecting literal segments and `{expr}` interpolations.
    pub(super) fn read_fstring(
        &mut self,
        quote: char,
        triple: bool,
    ) -> Result<Vec<FStringPart>, String> {
        let start_loc = self.loc();
        if triple {
            self.advance();
            self.advance();
        }
        let mut parts = Vec::new();
        let mut lit = String::new();

        loop {
            match self.peek() {
                None => {
                    return Err(format!("{}: unterminated f-string", start_loc.position()));
                }
                Some('\\') => {
                    self.advance();
                    match self.advance() {
                        Some('n') => lit.push('\n'),
                        Some('t') => lit.push('\t'),
                        Some('r') => lit.push('\r'),
                        Some('\\') => lit.push('\\'),
                        Some('\'') => lit.push('\''),
                        Some('"') => lit.push('"'),
                        Some('{') => lit.push('{'),
                        Some('}') => lit.push('}'),
                        Some(c) => {
                            lit.push('\\');
                            lit.push(c);
                        }
                        None => {
                            return Err(format!(
                                "{}: unterminated f-string escape",
                                start_loc.position()
                            ));
                        }
                    }
                }
                Some('{') if self.peek2() == Some('{') => {
                    // Escaped brace: {{ → {
                    self.advance();
                    self.advance();
                    lit.push('{');
                }
                Some('{') => {
                    self.advance();
                    if !lit.is_empty() {
                        parts.push(FStringPart::Lit(std::mem::take(&mut lit)));
                    }
                    // Read expression until matching }
                    let mut depth = 1u32;
                    let mut expr = String::new();
                    while depth > 0 {
                        match self.advance() {
                            None => {
                                return Err(format!(
                                    "{}: unterminated f-string expression",
                                    start_loc.position()
                                ));
                            }
                            Some('{') => {
                                depth += 1;
                                expr.push('{');
                            }
                            Some('}') => {
                                depth -= 1;
                                if depth > 0 {
                                    expr.push('}');
                                }
                            }
                            Some(c) => expr.push(c),
                        }
                    }
                    parts.push(FStringPart::Expr(expr));
                }
                Some('}') if self.peek2() == Some('}') => {
                    self.advance();
                    self.advance();
                    lit.push('}');
                }
                Some(c) if c == quote => {
                    self.advance();
                    if triple {
                        if self.peek() == Some(quote) && self.peek2() == Some(quote) {
                            self.advance();
                            self.advance();
                            if !lit.is_empty() {
                                parts.push(FStringPart::Lit(lit));
                            }
                            return Ok(parts);
                        }
                        lit.push(c);
                    } else {
                        if !lit.is_empty() {
                            parts.push(FStringPart::Lit(lit));
                        }
                        return Ok(parts);
                    }
                }
                Some(c) => {
                    self.advance();
                    lit.push(c);
                }
            }
        }
    }

    pub(super) fn read_number(&mut self) -> Result<PyToken, String> {
        let start = self.cursor.pos();
        let mut is_float = false;

        // Hex literal
        if self.peek() == Some('0') && matches!(self.peek2(), Some('x') | Some('X')) {
            self.advance();
            self.advance();
            while let Some(c) = self.peek() {
                if c.is_ascii_hexdigit() || c == '_' {
                    self.advance();
                } else {
                    break;
                }
            }
            let s: String = self
                .cursor
                .span(start)
                .iter()
                .filter(|c| **c != '_')
                .collect();
            let val =
                i64::from_str_radix(&s[2..], 16).map_err(|e| format!("bad hex literal: {}", e))?;
            return Ok(PyToken::Int(val));
        }

        // Binary literal
        if self.peek() == Some('0') && matches!(self.peek2(), Some('b') | Some('B')) {
            self.advance();
            self.advance();
            while let Some(c) = self.peek() {
                if c == '0' || c == '1' || c == '_' {
                    self.advance();
                } else {
                    break;
                }
            }
            let s: String = self
                .cursor
                .span(start)
                .iter()
                .filter(|c| **c != '_')
                .collect();
            let val = i64::from_str_radix(&s[2..], 2)
                .map_err(|e| format!("bad binary literal: {}", e))?;
            return Ok(PyToken::Int(val));
        }

        // Octal literal
        if self.peek() == Some('0') && matches!(self.peek2(), Some('o') | Some('O')) {
            self.advance();
            self.advance();
            while let Some(c) = self.peek() {
                if ('0'..='7').contains(&c) || c == '_' {
                    self.advance();
                } else {
                    break;
                }
            }
            let s: String = self
                .cursor
                .span(start)
                .iter()
                .filter(|c| **c != '_')
                .collect();
            let val =
                i64::from_str_radix(&s[2..], 8).map_err(|e| format!("bad octal literal: {}", e))?;
            return Ok(PyToken::Int(val));
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
            self.advance();
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

        let s: String = self
            .cursor
            .span(start)
            .iter()
            .filter(|c| **c != '_')
            .collect();

        if is_float {
            let val: f64 = s.parse().map_err(|e| format!("bad float literal: {}", e))?;
            Ok(PyToken::Float(val))
        } else {
            let val: i64 = s
                .parse()
                .map_err(|e| format!("bad integer literal: {}", e))?;
            Ok(PyToken::Int(val))
        }
    }

    pub(super) fn read_ident(&mut self) -> String {
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

    pub(super) fn keyword_or_ident(&self, s: &str) -> PyToken {
        match s {
            "def" => PyToken::Def,
            "return" => PyToken::Return,
            "if" => PyToken::If,
            "elif" => PyToken::Elif,
            "else" => PyToken::Else,
            "while" => PyToken::While,
            "for" => PyToken::For,
            "in" => PyToken::In,
            "and" => PyToken::And,
            "or" => PyToken::Or,
            "not" => PyToken::Not,
            "break" => PyToken::Break,
            "continue" => PyToken::Continue,
            "pass" => PyToken::Pass,
            "lambda" => PyToken::Lambda,
            "class" => PyToken::Class,
            "import" => PyToken::Import,
            "from" => PyToken::From,
            "as" => PyToken::As,
            "try" => PyToken::Try,
            "except" => PyToken::Except,
            "finally" => PyToken::Finally,
            "raise" => PyToken::Raise,
            "with" => PyToken::With,
            "yield" => PyToken::Yield,
            "assert" => PyToken::Assert,
            "del" => PyToken::Del,
            "global" => PyToken::Global,
            "nonlocal" => PyToken::Nonlocal,
            "is" => PyToken::Is,
            "True" => PyToken::True,
            "False" => PyToken::False,
            "None" => PyToken::None,
            _ => PyToken::Ident(s.to_string()),
        }
    }
}
