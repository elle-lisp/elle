use super::*;

impl LuaLexer {
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
                    Some('a') => s.push('\x07'), // bell
                    Some('b') => s.push('\x08'), // backspace
                    Some('f') => s.push('\x0C'), // form feed
                    Some('v') => s.push('\x0B'), // vertical tab
                    Some('\\') => s.push('\\'),
                    Some('\'') => s.push('\''),
                    Some('"') => s.push('"'),
                    Some('0') => s.push('\0'),
                    Some('x') => {
                        // \xNN hex escape
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
                    Some(c) if c.is_ascii_digit() => {
                        // \ddd decimal escape (1-3 digits)
                        let mut digits = String::new();
                        digits.push(c);
                        for _ in 0..2 {
                            if let Some(d) = self.peek() {
                                if d.is_ascii_digit() {
                                    digits.push(d);
                                    self.advance();
                                } else {
                                    break;
                                }
                            }
                        }
                        let val: u32 = digits.parse().unwrap();
                        if val > 255 {
                            return Err(format!(
                                "{}: decimal escape too large: \\{}",
                                start_loc.position(),
                                digits
                            ));
                        }
                        s.push(char::from(val as u8));
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
    /// Count `=` signs after current `[` and consume them + the closing `[`.
    /// Returns the level (number of `=` signs).
    pub(super) fn read_long_string_open(&mut self) -> usize {
        // We've already consumed the first `[`
        let mut level = 0;
        while self.peek() == Some('=') {
            self.advance();
            level += 1;
        }
        self.advance(); // skip closing `[`
        level
    }
    pub(super) fn read_long_string(&mut self, level: usize) -> Result<String, String> {
        let start_loc = self.loc();
        let mut s = std::string::String::new();
        // Skip optional leading newline
        if self.peek() == Some('\n') {
            self.advance();
        }
        loop {
            match self.advance() {
                None => {
                    return Err(format!(
                        "{}: unterminated long string",
                        start_loc.position()
                    ));
                }
                Some(']') => {
                    // Check for `]=*]` with matching level
                    let mut eq_count = 0;
                    while self.peek() == Some('=') {
                        eq_count += 1;
                        self.advance();
                    }
                    if eq_count == level && self.peek() == Some(']') {
                        self.advance();
                        return Ok(s);
                    }
                    // Not a match — push what we consumed
                    s.push(']');
                    for _ in 0..eq_count {
                        s.push('=');
                    }
                }
                Some(c) => s.push(c),
            }
        }
    }
    pub(super) fn read_number(&mut self) -> Result<LuaToken, String> {
        let start = self.cursor.pos();
        let mut is_float = false;

        // Hex literal
        if self.peek() == Some('0') && matches!(self.peek2(), Some('x') | Some('X')) {
            self.advance(); // 0
            self.advance(); // x
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
            return Ok(LuaToken::Int(val));
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
            Ok(LuaToken::Float(val))
        } else {
            let val: i64 = s
                .parse()
                .map_err(|e| format!("bad integer literal: {}", e))?;
            Ok(LuaToken::Int(val))
        }
    }
}
