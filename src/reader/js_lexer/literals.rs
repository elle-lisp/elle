use super::*;

impl JsLexer {
    pub(super) fn read_string(&mut self, quote: char) -> Result<String, String> {
        let start_loc = self.loc();
        self.advance(); // skip opening quote
        let mut s = std::string::String::new();
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
                    Some('`') => s.push('`'),
                    Some('$') => s.push('$'),
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
                        // \u{NNNN} or \uNNNN
                        if self.peek() == Some('{') {
                            self.advance();
                            let mut hex = String::new();
                            while self.peek().is_some_and(|c| c != '}') {
                                hex.push(self.advance().unwrap());
                            }
                            self.advance(); // }
                            let val = u32::from_str_radix(&hex, 16).map_err(|e| {
                                format!("{}: invalid \\u escape: {}", start_loc.position(), e)
                            })?;
                            s.push(char::from_u32(val).ok_or_else(|| {
                                format!("{}: invalid unicode codepoint", start_loc.position())
                            })?);
                        } else {
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
                    }
                    Some(c) => {
                        return Err(format!(
                            "{}: unknown escape sequence \\{}",
                            start_loc.position(),
                            c
                        ));
                    }
                },
                Some(c) if c == quote => return Ok(s),
                Some(c) => s.push(c),
            }
        }
    }

    /// Read a template literal after the opening backtick.
    /// Returns one or more tokens for the template segments.
    pub(super) fn read_template(&mut self) -> Result<Vec<JsToken>, String> {
        let start_loc = self.loc();
        self.advance(); // skip opening backtick
        let mut tokens = Vec::new();
        let mut s = String::new();
        let is_first = true;

        loop {
            match self.peek() {
                None => {
                    return Err(format!(
                        "{}: unterminated template literal",
                        start_loc.position()
                    ));
                }
                Some('\\') => {
                    self.advance();
                    match self.advance() {
                        Some('n') => s.push('\n'),
                        Some('t') => s.push('\t'),
                        Some('r') => s.push('\r'),
                        Some('\\') => s.push('\\'),
                        Some('`') => s.push('`'),
                        Some('$') => s.push('$'),
                        Some(c) => {
                            s.push('\\');
                            s.push(c);
                        }
                        None => {
                            return Err(format!(
                                "{}: unterminated template escape",
                                start_loc.position()
                            ));
                        }
                    }
                }
                Some('$') if self.peek2() == Some('{') => {
                    self.advance(); // $
                    self.advance(); // {
                    if is_first {
                        tokens.push(JsToken::TemplateHead(std::mem::take(&mut s)));
                    } else {
                        tokens.push(JsToken::TemplateMiddle(std::mem::take(&mut s)));
                    }
                    // The caller will handle tokenizing the expression until }
                    return Ok(tokens);
                }
                Some('`') => {
                    self.advance();
                    if is_first {
                        // No interpolation at all
                        tokens.push(JsToken::TemplateNoSub(s));
                    } else {
                        tokens.push(JsToken::TemplateTail(s));
                    }
                    return Ok(tokens);
                }
                Some(c) => {
                    self.advance();
                    s.push(c);
                }
            }
        }
    }

    /// Continue reading a template literal after a `}` closes an interpolation.
    pub(super) fn continue_template(&mut self) -> Result<Vec<JsToken>, String> {
        let start_loc = self.loc();
        let mut tokens = Vec::new();
        let mut s = String::new();

        loop {
            match self.peek() {
                None => {
                    return Err(format!(
                        "{}: unterminated template literal",
                        start_loc.position()
                    ));
                }
                Some('\\') => {
                    self.advance();
                    match self.advance() {
                        Some('n') => s.push('\n'),
                        Some('t') => s.push('\t'),
                        Some('r') => s.push('\r'),
                        Some('\\') => s.push('\\'),
                        Some('`') => s.push('`'),
                        Some('$') => s.push('$'),
                        Some(c) => {
                            s.push('\\');
                            s.push(c);
                        }
                        None => {
                            return Err(format!(
                                "{}: unterminated template escape",
                                start_loc.position()
                            ));
                        }
                    }
                }
                Some('$') if self.peek2() == Some('{') => {
                    self.advance(); // $
                    self.advance(); // {
                    tokens.push(JsToken::TemplateMiddle(std::mem::take(&mut s)));
                    return Ok(tokens);
                }
                Some('`') => {
                    self.advance();
                    tokens.push(JsToken::TemplateTail(s));
                    return Ok(tokens);
                }
                Some(c) => {
                    self.advance();
                    s.push(c);
                }
            }
        }
    }

    pub(super) fn read_number(&mut self) -> Result<JsToken, String> {
        let start = self.cursor.pos();
        let mut is_float = false;
        if let Some(val) = self
            .cursor
            .scan_radix_literal(crate::reader::scan::RadixPrefixes::HexOctalBinary)
        {
            return Ok(JsToken::Int(val?));
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
            self.advance(); // .
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
            Ok(JsToken::Float(val))
        } else {
            let val: i64 = s
                .parse()
                .map_err(|e| format!("bad integer literal: {}", e))?;
            Ok(JsToken::Int(val))
        }
    }

    pub(super) fn read_ident(&mut self) -> String {
        let start = self.cursor.pos();
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' || c == '$' {
                self.advance();
            } else {
                break;
            }
        }
        self.cursor.span(start).iter().collect()
    }

    pub(super) fn keyword_or_ident(&self, s: &str) -> JsToken {
        match s {
            "function" => JsToken::Function,
            "return" => JsToken::Return,
            "if" => JsToken::If,
            "else" => JsToken::Else,
            "while" => JsToken::While,
            "for" => JsToken::For,
            "of" => JsToken::Of,
            "in" => JsToken::In,
            "const" => JsToken::Const,
            "let" => JsToken::Let,
            "var" => JsToken::Var,
            "break" => JsToken::Break,
            "continue" => JsToken::Continue,
            "do" => JsToken::Do,
            "switch" => JsToken::Switch,
            "case" => JsToken::Case,
            "default" => JsToken::Default,
            "typeof" => JsToken::Typeof,
            "new" => JsToken::New,
            "throw" => JsToken::Throw,
            "try" => JsToken::Try,
            "catch" => JsToken::Catch,
            "finally" => JsToken::Finally,
            "true" => JsToken::True,
            "false" => JsToken::False,
            "null" => JsToken::Null,
            "undefined" => JsToken::Undefined,
            _ => JsToken::Ident(s.to_string()),
        }
    }
}
