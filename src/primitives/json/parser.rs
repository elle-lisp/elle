use crate::value::{list, TableKey, Value};
use std::collections::BTreeMap;
use std::rc::Rc;

/// JSON parser using recursive descent
pub struct JsonParser {
    input: Vec<char>,
    pos: usize,
}

impl JsonParser {
    pub fn new(input: &str) -> Self {
        JsonParser {
            input: input.chars().collect(),
            pos: 0,
        }
    }

    pub fn parse(&mut self) -> Result<Value, String> {
        self.skip_whitespace();
        if self.pos >= self.input.len() {
            return Err("Unexpected end of input: empty JSON".to_string());
        }
        let value = self.parse_value()?;
        self.skip_whitespace();
        if self.pos < self.input.len() {
            return Err(format!(
                "Unexpected trailing content at position {}: '{}'",
                self.pos, self.input[self.pos]
            ));
        }
        Ok(value)
    }

    fn parse_value(&mut self) -> Result<Value, String> {
        self.skip_whitespace();

        if self.pos >= self.input.len() {
            return Err("Unexpected end of input while parsing value".to_string());
        }

        match self.input[self.pos] {
            'n' => self.parse_literal("null", Value::Nil),
            't' => self.parse_literal("true", Value::Bool(true)),
            'f' => self.parse_literal("false", Value::Bool(false)),
            '"' => self.parse_string(),
            '[' => self.parse_array(),
            '{' => self.parse_object(),
            '-' | '0'..='9' => self.parse_number(),
            c => Err(format!(
                "Unexpected character '{}' at position {}",
                c, self.pos
            )),
        }
    }

    fn parse_literal(&mut self, literal: &str, value: Value) -> Result<Value, String> {
        let literal_chars: Vec<char> = literal.chars().collect();
        if self.pos + literal_chars.len() > self.input.len() {
            return Err(format!(
                "Unexpected end of input while parsing '{}'",
                literal
            ));
        }

        for (i, &expected) in literal_chars.iter().enumerate() {
            if self.input[self.pos + i] != expected {
                return Err(format!(
                    "Expected '{}' at position {}, got '{}'",
                    literal,
                    self.pos,
                    self.input[self.pos + i]
                ));
            }
        }

        self.pos += literal_chars.len();
        Ok(value)
    }

    fn parse_string(&mut self) -> Result<Value, String> {
        if self.input[self.pos] != '"' {
            return Err(format!(
                "Expected '\"' at position {}, got '{}'",
                self.pos, self.input[self.pos]
            ));
        }

        self.pos += 1;
        let mut result = String::new();

        while self.pos < self.input.len() {
            match self.input[self.pos] {
                '"' => {
                    self.pos += 1;
                    return Ok(Value::String(Rc::from(result)));
                }
                '\\' => {
                    self.pos += 1;
                    if self.pos >= self.input.len() {
                        return Err(format!(
                            "Unexpected end of input in string escape at position {}",
                            self.pos
                        ));
                    }

                    match self.input[self.pos] {
                        '"' => result.push('"'),
                        '\\' => result.push('\\'),
                        '/' => result.push('/'),
                        'b' => result.push('\u{0008}'),
                        'f' => result.push('\u{000C}'),
                        'n' => result.push('\n'),
                        'r' => result.push('\r'),
                        't' => result.push('\t'),
                        'u' => {
                            self.pos += 1;
                            if self.pos + 3 >= self.input.len() {
                                return Err(format!(
                                    "Unexpected end of input in unicode escape at position {}",
                                    self.pos
                                ));
                            }

                            let hex_str: String =
                                self.input[self.pos..self.pos + 4].iter().collect();
                            let code_point = u32::from_str_radix(&hex_str, 16).map_err(|_| {
                                format!("Invalid unicode escape sequence: \\u{}", hex_str)
                            })?;

                            // Handle UTF-16 surrogate pairs
                            if (0xD800..=0xDBFF).contains(&code_point) {
                                // High surrogate - look for low surrogate
                                self.pos += 4;
                                if self.pos + 5 < self.input.len()
                                    && self.input[self.pos] == '\\'
                                    && self.input[self.pos + 1] == 'u'
                                {
                                    self.pos += 2;
                                    let low_hex_str: String =
                                        self.input[self.pos..self.pos + 4].iter().collect();
                                    let low_code_point = u32::from_str_radix(&low_hex_str, 16)
                                        .map_err(|_| {
                                            format!(
                                                "Invalid unicode escape sequence: \\u{}",
                                                low_hex_str
                                            )
                                        })?;

                                    if (0xDC00..=0xDFFF).contains(&low_code_point) {
                                        // Valid surrogate pair
                                        let combined = 0x10000
                                            + (code_point - 0xD800) * 0x400
                                            + (low_code_point - 0xDC00);
                                        if let Some(ch) = char::from_u32(combined) {
                                            result.push(ch);
                                            self.pos += 3;
                                        } else {
                                            return Err(format!(
                                                "Invalid surrogate pair code point: U+{:04X}U+{:04X}",
                                                code_point, low_code_point
                                            ));
                                        }
                                    } else {
                                        return Err(format!(
                                            "High surrogate not followed by low surrogate at position {}",
                                            self.pos - 2
                                        ));
                                    }
                                } else {
                                    return Err(format!(
                                        "Lone high surrogate without low surrogate at position {}",
                                        self.pos - 4
                                    ));
                                }
                            } else if (0xDC00..=0xDFFF).contains(&code_point) {
                                // Low surrogate without high surrogate
                                return Err(format!(
                                    "Unexpected low surrogate at position {}",
                                    self.pos
                                ));
                            } else if let Some(ch) = char::from_u32(code_point) {
                                result.push(ch);
                                self.pos += 3;
                            } else {
                                return Err(format!(
                                    "Invalid unicode code point: U+{:04X}",
                                    code_point
                                ));
                            }
                        }
                        c => {
                            return Err(format!(
                                "Invalid escape sequence '\\{}' at position {}",
                                c, self.pos
                            ))
                        }
                    }
                    self.pos += 1;
                }
                c => {
                    result.push(c);
                    self.pos += 1;
                }
            }
        }

        Err(format!("Unterminated string at position {}", self.pos))
    }

    fn parse_number(&mut self) -> Result<Value, String> {
        let start = self.pos;

        // Optional minus
        if self.pos < self.input.len() && self.input[self.pos] == '-' {
            self.pos += 1;
        }

        // Integer part
        if self.pos >= self.input.len() {
            return Err("Unexpected end of input in number".to_string());
        }

        if self.input[self.pos] == '0' {
            self.pos += 1;
        } else if self.input[self.pos].is_ascii_digit() {
            while self.pos < self.input.len() && self.input[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        } else {
            return Err(format!(
                "Invalid number at position {}: expected digit",
                self.pos
            ));
        }

        // Check for decimal point or exponent
        let has_decimal = self.pos < self.input.len() && self.input[self.pos] == '.';
        let has_exponent = self.pos < self.input.len()
            && (self.input[self.pos] == 'e' || self.input[self.pos] == 'E');

        if has_decimal {
            self.pos += 1;
            if self.pos >= self.input.len() || !self.input[self.pos].is_ascii_digit() {
                return Err(format!(
                    "Expected digit after decimal point at position {}",
                    self.pos
                ));
            }
            while self.pos < self.input.len() && self.input[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }

        if self.pos < self.input.len()
            && (self.input[self.pos] == 'e' || self.input[self.pos] == 'E')
        {
            self.pos += 1;
            if self.pos < self.input.len()
                && (self.input[self.pos] == '+' || self.input[self.pos] == '-')
            {
                self.pos += 1;
            }
            if self.pos >= self.input.len() || !self.input[self.pos].is_ascii_digit() {
                return Err(format!(
                    "Expected digit in exponent at position {}",
                    self.pos
                ));
            }
            while self.pos < self.input.len() && self.input[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }

        let num_str: String = self.input[start..self.pos].iter().collect();

        if has_decimal || has_exponent {
            match num_str.parse::<f64>() {
                Ok(f) => Ok(Value::Float(f)),
                Err(_) => Err(format!("Invalid float: {}", num_str)),
            }
        } else {
            match num_str.parse::<i64>() {
                Ok(i) => Ok(Value::Int(i)),
                Err(_) => Err(format!("Invalid integer: {}", num_str)),
            }
        }
    }

    fn parse_array(&mut self) -> Result<Value, String> {
        if self.input[self.pos] != '[' {
            return Err(format!(
                "Expected '[' at position {}, got '{}'",
                self.pos, self.input[self.pos]
            ));
        }

        self.pos += 1;
        self.skip_whitespace();

        let mut elements = Vec::new();

        if self.pos < self.input.len() && self.input[self.pos] == ']' {
            self.pos += 1;
            return Ok(list(elements));
        }

        loop {
            elements.push(self.parse_value()?);
            self.skip_whitespace();

            if self.pos >= self.input.len() {
                return Err("Unexpected end of input in array".to_string());
            }

            match self.input[self.pos] {
                ',' => {
                    self.pos += 1;
                    self.skip_whitespace();
                }
                ']' => {
                    self.pos += 1;
                    return Ok(list(elements));
                }
                c => {
                    return Err(format!(
                        "Expected ',' or ']' in array at position {}, got '{}'",
                        self.pos, c
                    ))
                }
            }
        }
    }

    fn parse_object(&mut self) -> Result<Value, String> {
        if self.input[self.pos] != '{' {
            return Err(format!(
                "Expected '{{' at position {}, got '{}'",
                self.pos, self.input[self.pos]
            ));
        }

        self.pos += 1;
        self.skip_whitespace();

        let mut map = BTreeMap::new();

        if self.pos < self.input.len() && self.input[self.pos] == '}' {
            self.pos += 1;
            return Ok(Value::Table(Rc::new(std::cell::RefCell::new(map))));
        }

        loop {
            self.skip_whitespace();

            // Parse key (must be string)
            if self.pos >= self.input.len() || self.input[self.pos] != '"' {
                return Err(format!(
                    "Expected string key in object at position {}",
                    self.pos
                ));
            }

            let key_value = self.parse_string()?;
            let key = match key_value {
                Value::String(s) => TableKey::String(s.to_string()),
                _ => unreachable!(),
            };

            self.skip_whitespace();

            // Expect colon
            if self.pos >= self.input.len() || self.input[self.pos] != ':' {
                return Err(format!(
                    "Expected ':' after object key at position {}",
                    self.pos
                ));
            }

            self.pos += 1;
            self.skip_whitespace();

            // Parse value
            let value = self.parse_value()?;
            map.insert(key, value);

            self.skip_whitespace();

            if self.pos >= self.input.len() {
                return Err("Unexpected end of input in object".to_string());
            }

            match self.input[self.pos] {
                ',' => {
                    self.pos += 1;
                }
                '}' => {
                    self.pos += 1;
                    return Ok(Value::Table(Rc::new(std::cell::RefCell::new(map))));
                }
                c => {
                    return Err(format!(
                        "Expected ',' or '}}' in object at position {}, got '{}'",
                        self.pos, c
                    ))
                }
            }
        }
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.input.len() {
            match self.input[self.pos] {
                ' ' | '\t' | '\n' | '\r' => self.pos += 1,
                _ => break,
            }
        }
    }
}
