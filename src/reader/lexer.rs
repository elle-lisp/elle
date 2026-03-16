use super::token::{SourceLoc, Token, TokenWithLoc};

/// Fast delimiter check - O(1) instead of string contains O(n)
/// Checks if a character is a Lisp delimiter
#[inline]
fn is_delimiter(c: char) -> bool {
    matches!(
        c,
        '(' | ')' | '[' | ']' | '{' | '}' | '\'' | '`' | ',' | ':' | '@' | ';' | '|'
    )
}

/// Check if a character can start a symbol name (for qualified name parsing).
/// Used to determine if `module:name` should be read as a single qualified symbol.
#[inline]
fn is_symbol_start(c: char) -> bool {
    c.is_alphabetic() || matches!(c, '_' | '-' | '+' | '*' | '/' | '!' | '?' | '<' | '>' | '=')
}

/// Validate that a digit-body string does not have leading, trailing, or consecutive underscores.
/// Returns the stripped string (underscores removed) if valid, or an error message.
/// `context` is the full raw literal text used in the error message.
fn validate_and_strip_underscores(s: &str, context: &str) -> Result<String, String> {
    if s.starts_with('_') || s.ends_with('_') || s.contains("__") {
        return Err(format!("Invalid underscore in numeric literal: {context}"));
    }
    Ok(s.replace('_', ""))
}

pub struct Lexer<'a> {
    input: &'a str,
    bytes: &'a [u8],
    pos: usize,
    line: usize,
    col: usize,
    file: String,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Lexer {
            input,
            bytes: input.as_bytes(),
            pos: 0,
            line: 1,
            col: 1,
            file: "<unknown>".to_string(),
        }
    }

    pub fn with_file(input: &'a str, file: impl Into<String>) -> Self {
        Lexer {
            input,
            bytes: input.as_bytes(),
            pos: 0,
            line: 1,
            col: 1,
            file: file.into(),
        }
    }

    fn get_loc(&self) -> SourceLoc {
        SourceLoc::new(&self.file, self.line, self.col)
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
            } else if c == '#' {
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
        let mut raw = String::new();
        let mut sign = String::new();

        // Step 1: consume optional sign
        if let Some(c) = self.current() {
            if c == '+' || c == '-' {
                sign.push(c);
                raw.push(c);
                self.advance();
            }
        }

        // Step 2: detect base prefix (0x, 0o, 0b and uppercase variants)
        let mut base: u32 = 10;
        if self.current() == Some('0') {
            let next = self.peek(1);
            match next {
                Some('x') | Some('X') | Some('o') | Some('O') | Some('b') | Some('B') => {
                    raw.push('0');
                    self.advance(); // consume '0'
                    let p = self.current().unwrap();
                    raw.push(p);
                    self.advance(); // consume prefix char
                    match p.to_ascii_lowercase() {
                        'x' => base = 16,
                        'o' => base = 8,
                        'b' => base = 2,
                        _ => unreachable!(),
                    }
                }
                _ => {}
            }
        }
        let base_name = match base {
            16 => "hexadecimal",
            8 => "octal",
            2 => "binary",
            _ => "decimal",
        };

        // Step 3: collect digit body
        let mut body = String::new();
        let mut has_dot = false;
        let mut has_exp = false;

        if base != 10 {
            // Prefixed literal: consume only valid digit chars for the base
            while let Some(c) = self.current() {
                let valid = match base {
                    16 => c.is_ascii_hexdigit() || c == '_',
                    8 => matches!(c, '0'..='7' | '_'),
                    2 => matches!(c, '0' | '1' | '_'),
                    _ => unreachable!(),
                };
                if valid {
                    body.push(c);
                    raw.push(c);
                    self.advance();
                } else {
                    break;
                }
            }
            // body must not be empty
            if body.is_empty() {
                return Err(format!("Invalid {base_name} integer: {raw}"));
            }
            // The next character must not be an alphanumeric digit — if it is,
            // it's an invalid digit for this base (e.g. '2' after 0b1 is an error).
            if matches!(self.current(), Some(c) if c.is_ascii_alphanumeric()) {
                let bad = self.current().unwrap();
                return Err(format!("Invalid {base_name} integer: {raw}{bad}"));
            }
        } else {
            // Decimal: consume leading digits
            while let Some(c) = self.current() {
                if c.is_ascii_digit() || c == '_' {
                    body.push(c);
                    raw.push(c);
                    self.advance();
                } else {
                    break;
                }
            }

            // Optional fractional part
            if self.current() == Some('.') {
                // Check: character immediately before '.' must not be '_'
                if body.ends_with('_') {
                    return Err(format!("Invalid underscore in numeric literal: {raw}."));
                }
                // Peek: character immediately after '.' must not be '_'
                if self.peek(1) == Some('_') {
                    return Err(format!("Invalid underscore in numeric literal: {raw}._"));
                }
                has_dot = true;
                body.push('.');
                raw.push('.');
                self.advance();
                while let Some(c) = self.current() {
                    if c.is_ascii_digit() || c == '_' {
                        body.push(c);
                        raw.push(c);
                        self.advance();
                    } else {
                        break;
                    }
                }
            }

            // Optional exponent part (decimal only)
            if matches!(self.current(), Some('e') | Some('E')) {
                // Check: character immediately before 'e'/'E' must not be '_'
                if body.ends_with('_') {
                    return Err(format!("Invalid underscore in numeric literal: {raw}"));
                }
                has_exp = true;
                let e_char = self.current().unwrap();
                body.push(e_char);
                raw.push(e_char);
                self.advance();
                // Optional exponent sign
                if matches!(self.current(), Some('+') | Some('-')) {
                    let sign_char = self.current().unwrap();
                    body.push(sign_char);
                    raw.push(sign_char);
                    self.advance();
                }
                // Check: character immediately after 'e'/'E' (or sign) must not be '_'
                if self.current() == Some('_') {
                    return Err(format!("Invalid underscore in numeric literal: {raw}_"));
                }
                // Must have at least one exponent digit
                if !matches!(self.current(), Some(c) if c.is_ascii_digit()) {
                    return Err(format!("Invalid float: {raw}"));
                }
                while let Some(c) = self.current() {
                    if c.is_ascii_digit() || c == '_' {
                        body.push(c);
                        raw.push(c);
                        self.advance();
                    } else {
                        break;
                    }
                }
            }
        }

        // Step 4: validate and strip underscores from body
        let stripped_body = validate_and_strip_underscores(&body, &raw)?;

        // Step 5: parse
        if base != 10 {
            let n = i64::from_str_radix(&stripped_body, base)
                .map_err(|_| format!("Invalid {base_name} integer: {raw}"))?;
            if sign == "-" {
                Ok(Token::Integer(-n))
            } else {
                Ok(Token::Integer(n))
            }
        } else if has_dot || has_exp {
            let full = format!("{sign}{stripped_body}");
            full.parse::<f64>()
                .map(Token::Float)
                .map_err(|_| format!("Invalid float: {raw}"))
        } else {
            let full = format!("{sign}{stripped_body}");
            full.parse::<i64>()
                .map(Token::Integer)
                .map_err(|_| format!("Invalid integer: {raw}"))
        }
    }

    /// Read a symbol and return a slice of the original input.
    /// Handles qualified names like `module:name` as a single symbol.
    fn read_symbol(&mut self) -> (usize, usize) {
        let start = self.pos;
        while let Some(c) = self.current() {
            // Use fast delimiter check instead of string contains()
            if c.is_whitespace() || is_delimiter(c) {
                // Check for qualified name: if we hit ':' and next char can start a symbol,
                // continue reading as a qualified name
                if c == ':' {
                    if let Some(next) = self.peek(1) {
                        if is_symbol_start(next) {
                            // Include the colon and continue reading
                            self.advance(); // consume ':'
                            continue;
                        }
                    }
                }
                break;
            }
            self.advance();
        }
        (start, self.pos)
    }

    pub fn next_token_with_loc(&mut self) -> Result<Option<TokenWithLoc<'a>>, String> {
        self.skip_whitespace();
        let loc = self.get_loc();
        let start_pos = self.pos;

        match self.current() {
            None => Ok(None),
            Some('(') => {
                self.advance();
                Ok(Some(TokenWithLoc {
                    token: Token::LeftParen,
                    loc,
                    len: self.pos - start_pos,
                    byte_offset: start_pos,
                }))
            }
            Some(')') => {
                self.advance();
                Ok(Some(TokenWithLoc {
                    token: Token::RightParen,
                    loc,
                    len: self.pos - start_pos,
                    byte_offset: start_pos,
                }))
            }
            Some('[') => {
                self.advance();
                Ok(Some(TokenWithLoc {
                    token: Token::LeftBracket,
                    loc,
                    len: self.pos - start_pos,
                    byte_offset: start_pos,
                }))
            }
            Some(']') => {
                self.advance();
                Ok(Some(TokenWithLoc {
                    token: Token::RightBracket,
                    loc,
                    len: self.pos - start_pos,
                    byte_offset: start_pos,
                }))
            }
            Some('{') => {
                self.advance();
                Ok(Some(TokenWithLoc {
                    token: Token::LeftBrace,
                    loc,
                    len: self.pos - start_pos,
                    byte_offset: start_pos,
                }))
            }
            Some('}') => {
                self.advance();
                Ok(Some(TokenWithLoc {
                    token: Token::RightBrace,
                    loc,
                    len: self.pos - start_pos,
                    byte_offset: start_pos,
                }))
            }
            Some('\'') => {
                self.advance();
                Ok(Some(TokenWithLoc {
                    token: Token::Quote,
                    loc,
                    len: self.pos - start_pos,
                    byte_offset: start_pos,
                }))
            }
            Some('`') => {
                self.advance();
                Ok(Some(TokenWithLoc {
                    token: Token::Quasiquote,
                    loc,
                    len: self.pos - start_pos,
                    byte_offset: start_pos,
                }))
            }
            Some(',') => {
                self.advance();
                if self.current() == Some(';') {
                    self.advance();
                    Ok(Some(TokenWithLoc {
                        token: Token::UnquoteSplicing,
                        loc,
                        len: self.pos - start_pos,
                        byte_offset: start_pos,
                    }))
                } else {
                    Ok(Some(TokenWithLoc {
                        token: Token::Unquote,
                        loc,
                        len: self.pos - start_pos,
                        byte_offset: start_pos,
                    }))
                }
            }
            Some(';') => {
                self.advance();
                Ok(Some(TokenWithLoc {
                    token: Token::Splice,
                    loc,
                    len: self.pos - start_pos,
                    byte_offset: start_pos,
                }))
            }
            Some('|') => {
                self.advance();
                Ok(Some(TokenWithLoc {
                    token: Token::Pipe,
                    loc,
                    len: self.pos - start_pos,
                    byte_offset: start_pos,
                }))
            }
            Some('@') => {
                self.advance();
                match self.current() {
                    // @| → mutable set literal delimiter
                    Some('|') => {
                        self.advance();
                        Ok(Some(TokenWithLoc {
                            token: Token::AtPipe,
                            loc,
                            len: self.pos - start_pos,
                            byte_offset: start_pos,
                        }))
                    }
                    // @symbol → symbol with @ prefix (e.g. @set, @array)
                    Some(c) if is_symbol_start(c) => {
                        let (_, end) = self.read_symbol();
                        let name = self.slice(start_pos, end);
                        Ok(Some(TokenWithLoc {
                            token: Token::Symbol(name),
                            loc,
                            len: self.pos - start_pos,
                            byte_offset: start_pos,
                        }))
                    }
                    // @[, @{, @" → collection sugar
                    _ => Ok(Some(TokenWithLoc {
                        token: Token::ListSugar,
                        loc,
                        len: self.pos - start_pos,
                        byte_offset: start_pos,
                    })),
                }
            }
            Some(':') => {
                self.advance();
                // Allow :@name for mutable type keywords (e.g. :@set, :@array)
                let at_prefix = if self.current() == Some('@') {
                    self.advance();
                    true
                } else {
                    false
                };
                // Read keyword - must be followed by symbol characters
                let (start, end) = self.read_symbol();
                if start == end {
                    Err("Invalid keyword: expected symbol after :".to_string())
                } else {
                    let keyword = if at_prefix {
                        // The @ was already consumed, so we need to include it in the keyword name.
                        // Since @ is at position (start - 1) in the source, we can slice from there.
                        self.slice(start - 1, end)
                    } else {
                        self.slice(start, end)
                    };
                    Ok(Some(TokenWithLoc {
                        token: Token::Keyword(keyword),
                        loc,
                        len: self.pos - start_pos,
                        byte_offset: start_pos,
                    }))
                }
            }
            Some('"') => {
                let token = Token::String(self.read_string()?);
                let len = self.pos - start_pos;
                Ok(Some(TokenWithLoc {
                    token,
                    loc,
                    len,
                    byte_offset: start_pos,
                }))
            }
            Some(c) if c.is_ascii_digit() || c == '-' || c == '+' => {
                // Check if it's a number or symbol
                if let Some(next) = self.peek(1) {
                    if (c == '-' || c == '+') && !next.is_ascii_digit() {
                        let (start, end) = self.read_symbol();
                        let sym = self.slice(start, end);
                        Ok(Some(TokenWithLoc {
                            token: Token::Symbol(sym),
                            loc,
                            len: self.pos - start_pos,
                            byte_offset: start_pos,
                        }))
                    } else {
                        let token = self.read_number()?;
                        let len = self.pos - start_pos;
                        Ok(Some(TokenWithLoc {
                            token,
                            loc,
                            len,
                            byte_offset: start_pos,
                        }))
                    }
                } else if c == '-' || c == '+' {
                    let (start, end) = self.read_symbol();
                    let sym = self.slice(start, end);
                    Ok(Some(TokenWithLoc {
                        token: Token::Symbol(sym),
                        loc,
                        len: self.pos - start_pos,
                        byte_offset: start_pos,
                    }))
                } else {
                    let token = self.read_number()?;
                    let len = self.pos - start_pos;
                    Ok(Some(TokenWithLoc {
                        token,
                        loc,
                        len,
                        byte_offset: start_pos,
                    }))
                }
            }

            Some(_) => {
                let (start, end) = self.read_symbol();
                let sym = self.slice(start, end);
                let len = self.pos - start_pos;
                if sym == "nil" {
                    Ok(Some(TokenWithLoc {
                        token: Token::Nil,
                        loc,
                        len,
                        byte_offset: start_pos,
                    }))
                } else if sym == "true" {
                    Ok(Some(TokenWithLoc {
                        token: Token::Bool(true),
                        loc,
                        len,
                        byte_offset: start_pos,
                    }))
                } else if sym == "false" {
                    Ok(Some(TokenWithLoc {
                        token: Token::Bool(false),
                        loc,
                        len,
                        byte_offset: start_pos,
                    }))
                } else {
                    Ok(Some(TokenWithLoc {
                        token: Token::Symbol(sym),
                        loc,
                        len,
                        byte_offset: start_pos,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn lex_single(input: &str) -> Token<'_> {
        let mut lexer = Lexer::new(input);
        lexer.next_token().unwrap().unwrap()
    }

    fn lex_err(input: &str) -> String {
        let mut lexer = Lexer::new(input);
        lexer.next_token().unwrap_err()
    }

    #[test]
    fn true_word_lexes_as_bool() {
        assert!(matches!(lex_single("true"), Token::Bool(true)));
    }

    #[test]
    fn false_word_lexes_as_bool() {
        assert!(matches!(lex_single("false"), Token::Bool(false)));
    }

    #[test]
    fn true_question_mark_is_symbol() {
        assert!(matches!(lex_single("true?"), Token::Symbol("true?")));
    }

    #[test]
    fn trueish_is_symbol() {
        assert!(matches!(lex_single("trueish"), Token::Symbol("trueish")));
    }

    #[test]
    fn false_positive_is_symbol() {
        assert!(matches!(
            lex_single("false-positive"),
            Token::Symbol("false-positive")
        ));
    }

    #[test]
    fn truetrue_is_symbol() {
        assert!(matches!(lex_single("truetrue"), Token::Symbol("truetrue")));
    }

    // ---- Hex literals ----

    #[test]
    fn hex_lowercase_prefix() {
        assert!(matches!(lex_single("0xff"), Token::Integer(255)));
    }

    #[test]
    fn hex_uppercase_prefix() {
        assert!(matches!(lex_single("0XFF"), Token::Integer(255)));
    }

    #[test]
    fn hex_mixed_case_digits() {
        assert!(matches!(lex_single("0x1A2b"), Token::Integer(0x1A2B)));
    }

    #[test]
    fn hex_zero() {
        assert!(matches!(lex_single("0x0"), Token::Integer(0)));
    }

    #[test]
    fn hex_max_positive() {
        // 0x7FFFFFFFFFFFFFFF == i64::MAX
        assert!(matches!(
            lex_single("0x7FFFFFFFFFFFFFFF"),
            Token::Integer(i64::MAX)
        ));
    }

    #[test]
    fn hex_with_underscore() {
        assert!(matches!(lex_single("0xFF_FF"), Token::Integer(0xFFFF)));
    }

    #[test]
    fn hex_positive_sign() {
        assert!(matches!(lex_single("+0xFF"), Token::Integer(255)));
    }

    // ---- Octal literals ----

    #[test]
    fn octal_lowercase_prefix() {
        assert!(matches!(lex_single("0o755"), Token::Integer(493)));
    }

    #[test]
    fn octal_uppercase_prefix() {
        assert!(matches!(lex_single("0O755"), Token::Integer(493)));
    }

    #[test]
    fn octal_zero() {
        assert!(matches!(lex_single("0o0"), Token::Integer(0)));
    }

    #[test]
    fn octal_with_underscore() {
        assert!(matches!(lex_single("0o7_5_5"), Token::Integer(493)));
    }

    // ---- Binary literals ----

    #[test]
    fn binary_lowercase_prefix() {
        assert!(matches!(lex_single("0b1010"), Token::Integer(10)));
    }

    #[test]
    fn binary_uppercase_prefix() {
        assert!(matches!(lex_single("0B1010"), Token::Integer(10)));
    }

    #[test]
    fn binary_zero() {
        assert!(matches!(lex_single("0b0"), Token::Integer(0)));
    }

    #[test]
    fn binary_with_underscore() {
        assert!(matches!(
            lex_single("0b1010_1010"),
            Token::Integer(0b10101010)
        ));
    }

    // ---- Decimal with underscores ----

    #[test]
    fn decimal_underscore_integer() {
        assert!(matches!(lex_single("1_000_000"), Token::Integer(1_000_000)));
    }

    #[test]
    fn decimal_underscore_float() {
        assert!(matches!(lex_single("1_000.5_5"), Token::Float(f) if (f - 1000.55).abs() < 1e-9));
    }

    // ---- Scientific notation (bug fix) ----

    #[test]
    fn scientific_with_dot() {
        assert!(matches!(lex_single("1.5e10"), Token::Float(f) if (f - 1.5e10).abs() < 1.0));
    }

    #[test]
    fn scientific_without_dot() {
        assert!(matches!(lex_single("1e10"), Token::Float(f) if (f - 1e10).abs() < 1.0));
    }

    #[test]
    fn scientific_negative_exponent() {
        assert!(matches!(lex_single("2.3e-5"), Token::Float(f) if (f - 2.3e-5).abs() < 1e-15));
    }

    #[test]
    fn scientific_positive_exponent() {
        assert!(matches!(lex_single("1e+10"), Token::Float(f) if (f - 1e10).abs() < 1.0));
    }

    #[test]
    fn scientific_uppercase_e() {
        assert!(matches!(lex_single("1.5E10"), Token::Float(f) if (f - 1.5e10).abs() < 1.0));
    }

    #[test]
    fn scientific_underscore_in_exponent() {
        assert!(matches!(lex_single("1.5e1_0"), Token::Float(f) if (f - 1.5e10).abs() < 1.0));
    }

    #[test]
    fn scientific_positive_sign() {
        assert!(matches!(lex_single("+1.5e10"), Token::Float(f) if (f - 1.5e10).abs() < 1.0));
    }

    // ---- Backward compatibility ----

    #[test]
    fn decimal_plain_integer() {
        assert!(matches!(lex_single("42"), Token::Integer(42)));
    }

    #[test]
    fn decimal_plain_float() {
        assert!(matches!(lex_single("2.71"), Token::Float(f) if (f - 2.71_f64).abs() < 1e-9));
    }

    #[test]
    fn decimal_negative_integer() {
        assert!(matches!(lex_single("-42"), Token::Integer(-42)));
    }

    #[test]
    fn decimal_zero() {
        assert!(matches!(lex_single("0"), Token::Integer(0)));
    }

    #[test]
    fn decimal_leading_zero_stays_decimal() {
        // 042 is decimal 42, not octal
        assert!(matches!(lex_single("042"), Token::Integer(42)));
    }

    // ---- Error cases ----

    #[test]
    fn hex_invalid_digit_error() {
        let e = lex_err("0xGG");
        assert!(e.contains("Invalid hexadecimal integer"), "got: {e}");
    }

    #[test]
    fn hex_empty_body_error() {
        let e = lex_err("0x");
        assert!(e.contains("Invalid hexadecimal integer"), "got: {e}");
    }

    #[test]
    fn octal_invalid_digit_error() {
        let e = lex_err("0o888");
        assert!(e.contains("Invalid octal integer"), "got: {e}");
    }

    #[test]
    fn octal_empty_body_error() {
        let e = lex_err("0o");
        assert!(e.contains("Invalid octal integer"), "got: {e}");
    }

    #[test]
    fn binary_invalid_digit_error() {
        let e = lex_err("0b123");
        assert!(e.contains("Invalid binary integer"), "got: {e}");
    }

    #[test]
    fn binary_empty_body_error() {
        let e = lex_err("0b");
        assert!(e.contains("Invalid binary integer"), "got: {e}");
    }

    #[test]
    fn underscore_consecutive_error() {
        let e = lex_err("1__000");
        assert!(e.contains("Invalid underscore"), "got: {e}");
    }

    #[test]
    fn underscore_trailing_error() {
        let e = lex_err("1_");
        assert!(e.contains("Invalid underscore"), "got: {e}");
    }

    #[test]
    fn underscore_leading_after_hex_prefix_error() {
        let e = lex_err("0x_FF");
        assert!(e.contains("Invalid underscore"), "got: {e}");
    }

    #[test]
    fn underscore_before_dot_error() {
        let e = lex_err("1_.5");
        assert!(e.contains("Invalid underscore"), "got: {e}");
    }

    #[test]
    fn underscore_after_dot_error() {
        let e = lex_err("1._5");
        assert!(e.contains("Invalid underscore"), "got: {e}");
    }

    #[test]
    fn underscore_before_exponent_marker_error() {
        let e = lex_err("1_e10");
        assert!(e.contains("Invalid underscore"), "got: {e}");
    }

    #[test]
    fn underscore_after_exponent_marker_error() {
        let e = lex_err("1e_10");
        assert!(e.contains("Invalid underscore"), "got: {e}");
    }

    #[test]
    fn scientific_missing_exponent_digits_error() {
        let e = lex_err("1.5e");
        assert!(e.contains("Invalid float"), "got: {e}");
    }

    #[test]
    fn scientific_sign_no_exponent_digits_error_pos() {
        let e = lex_err("1.5e+");
        assert!(e.contains("Invalid float"), "got: {e}");
    }

    #[test]
    fn scientific_sign_no_exponent_digits_error_neg() {
        let e = lex_err("1.5e-");
        assert!(e.contains("Invalid float"), "got: {e}");
    }
}
