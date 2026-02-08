//! JSON parsing and serialization primitives
//!
//! Provides hand-written recursive descent JSON parser and serializer.
//! No external JSON libraries - all implemented directly.

use crate::value::{list, TableKey, Value};
use std::collections::BTreeMap;
use std::rc::Rc;

/// Parse a JSON string into Elle values
pub fn prim_json_parse(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("json-parse requires exactly 1 argument".to_string());
    }

    let json_str = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err("json-parse requires a string argument".to_string()),
    };

    let mut parser = JsonParser::new(json_str);
    parser.parse()
}

/// Serialize an Elle value to compact JSON
pub fn prim_json_serialize(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("json-serialize requires exactly 1 argument".to_string());
    }

    let json_str = serialize_value(&args[0])?;
    Ok(Value::String(Rc::from(json_str)))
}

/// Serialize an Elle value to pretty-printed JSON with 2-space indentation
pub fn prim_json_serialize_pretty(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("json-serialize-pretty requires exactly 1 argument".to_string());
    }

    let json_str = serialize_value_pretty(&args[0], 0)?;
    Ok(Value::String(Rc::from(json_str)))
}

/// JSON parser using recursive descent
struct JsonParser {
    input: Vec<char>,
    pos: usize,
}

impl JsonParser {
    fn new(input: &str) -> Self {
        JsonParser {
            input: input.chars().collect(),
            pos: 0,
        }
    }

    fn parse(&mut self) -> Result<Value, String> {
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

/// Serialize a value to compact JSON
fn serialize_value(value: &Value) -> Result<String, String> {
    match value {
        Value::Nil => Ok("null".to_string()),
        Value::Bool(b) => Ok(if *b { "true" } else { "false" }.to_string()),
        Value::Int(i) => Ok(i.to_string()),
        Value::Float(f) => {
            // Guard against non-finite values
            if f.is_nan() || f.is_infinite() {
                return Err("Cannot serialize non-finite float value to JSON".to_string());
            }
            // Ensure floats always have a decimal point
            let s = f.to_string();
            if s.contains('.') || s.contains('e') || s.contains('E') {
                Ok(s)
            } else {
                Ok(format!("{}.0", s))
            }
        }
        Value::String(s) => Ok(escape_json_string(s)),
        Value::Cons(_) => {
            // Convert list to array
            let vec = value.list_to_vec()?;
            let elements: Result<Vec<String>, String> = vec.iter().map(serialize_value).collect();
            Ok(format!("[{}]", elements?.join(",")))
        }
        Value::Vector(v) => {
            let elements: Result<Vec<String>, String> = v.iter().map(serialize_value).collect();
            Ok(format!("[{}]", elements?.join(",")))
        }
        Value::Table(t) => {
            let table = t.borrow();
            let mut pairs = Vec::new();
            for (k, v) in table.iter() {
                let key_str = match k {
                    TableKey::String(s) => escape_json_string(s),
                    _ => {
                        return Err("Table keys must be strings for JSON serialization".to_string())
                    }
                };
                let val_str = serialize_value(v)?;
                pairs.push(format!("{}:{}", key_str, val_str));
            }
            Ok(format!("{{{}}}", pairs.join(",")))
        }
        Value::Struct(s) => {
            let mut pairs = Vec::new();
            for (k, v) in s.iter() {
                let key_str = match k {
                    TableKey::String(s) => escape_json_string(s),
                    _ => {
                        return Err("Struct keys must be strings for JSON serialization".to_string())
                    }
                };
                let val_str = serialize_value(v)?;
                pairs.push(format!("{}:{}", key_str, val_str));
            }
            Ok(format!("{{{}}}", pairs.join(",")))
        }
        Value::Keyword(_id) => {
            // Serialize keywords as strings (without the colon prefix)
            // Note: We don't have access to the symbol table here, so we'll use the ID
            // In practice, keywords should be converted to strings before serialization
            Err("Cannot serialize keyword without symbol table context".to_string())
        }
        Value::Closure(_) => Err("Cannot serialize closures to JSON".to_string()),
        Value::NativeFn(_) => Err("Cannot serialize native functions to JSON".to_string()),
        Value::Symbol(_) => Err("Cannot serialize symbols to JSON".to_string()),
        Value::LibHandle(_) => Err("Cannot serialize library handles to JSON".to_string()),
        Value::CHandle(_) => Err("Cannot serialize C handles to JSON".to_string()),
        Value::Exception(_) => Err("Cannot serialize exceptions to JSON".to_string()),
        Value::Condition(_) => Err("Cannot serialize conditions to JSON".to_string()),
    }
}

/// Serialize a value to pretty-printed JSON with indentation
fn serialize_value_pretty(value: &Value, indent_level: usize) -> Result<String, String> {
    let indent = "  ".repeat(indent_level);
    let next_indent = "  ".repeat(indent_level + 1);

    match value {
        Value::Nil => Ok("null".to_string()),
        Value::Bool(b) => Ok(if *b { "true" } else { "false" }.to_string()),
        Value::Int(i) => Ok(i.to_string()),
        Value::Float(f) => {
            // Guard against non-finite values
            if f.is_nan() || f.is_infinite() {
                return Err("Cannot serialize non-finite float value to JSON".to_string());
            }
            let s = f.to_string();
            if s.contains('.') || s.contains('e') || s.contains('E') {
                Ok(s)
            } else {
                Ok(format!("{}.0", s))
            }
        }
        Value::String(s) => Ok(escape_json_string(s)),
        Value::Cons(_) => {
            let vec = value.list_to_vec()?;
            if vec.is_empty() {
                return Ok("[]".to_string());
            }
            let elements: Result<Vec<String>, String> = vec
                .iter()
                .map(|v| serialize_value_pretty(v, indent_level + 1))
                .collect();
            Ok(format!(
                "[\n{}{}\n{}]",
                next_indent,
                elements?.join(&format!(",\n{}", next_indent)),
                indent
            ))
        }
        Value::Vector(v) => {
            if v.is_empty() {
                return Ok("[]".to_string());
            }
            let elements: Result<Vec<String>, String> = v
                .iter()
                .map(|val| serialize_value_pretty(val, indent_level + 1))
                .collect();
            Ok(format!(
                "[\n{}{}\n{}]",
                next_indent,
                elements?.join(&format!(",\n{}", next_indent)),
                indent
            ))
        }
        Value::Table(t) => {
            let table = t.borrow();
            if table.is_empty() {
                return Ok("{}".to_string());
            }
            let mut pairs = Vec::new();
            for (k, v) in table.iter() {
                let key_str = match k {
                    TableKey::String(s) => escape_json_string(s),
                    _ => {
                        return Err("Table keys must be strings for JSON serialization".to_string())
                    }
                };
                let val_str = serialize_value_pretty(v, indent_level + 1)?;
                pairs.push(format!("{}: {}", key_str, val_str));
            }
            Ok(format!(
                "{{\n{}{}\n{}}}",
                next_indent,
                pairs.join(&format!(",\n{}", next_indent)),
                indent
            ))
        }
        Value::Struct(s) => {
            if s.is_empty() {
                return Ok("{}".to_string());
            }
            let mut pairs = Vec::new();
            for (k, v) in s.iter() {
                let key_str = match k {
                    TableKey::String(s) => escape_json_string(s),
                    _ => {
                        return Err("Struct keys must be strings for JSON serialization".to_string())
                    }
                };
                let val_str = serialize_value_pretty(v, indent_level + 1)?;
                pairs.push(format!("{}: {}", key_str, val_str));
            }
            Ok(format!(
                "{{\n{}{}\n{}}}",
                next_indent,
                pairs.join(&format!(",\n{}", next_indent)),
                indent
            ))
        }
        Value::Keyword(_) => {
            Err("Cannot serialize keyword without symbol table context".to_string())
        }
        Value::Closure(_) => Err("Cannot serialize closures to JSON".to_string()),
        Value::NativeFn(_) => Err("Cannot serialize native functions to JSON".to_string()),
        Value::Symbol(_) => Err("Cannot serialize symbols to JSON".to_string()),
        Value::LibHandle(_) => Err("Cannot serialize library handles to JSON".to_string()),
        Value::CHandle(_) => Err("Cannot serialize C handles to JSON".to_string()),
        Value::Exception(_) => Err("Cannot serialize exceptions to JSON".to_string()),
        Value::Condition(_) => Err("Cannot serialize conditions to JSON".to_string()),
    }
}

/// Escape a string for JSON output
fn escape_json_string(s: &str) -> String {
    let mut result = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\u{0008}' => result.push_str("\\b"),
            '\u{000C}' => result.push_str("\\f"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            c if c.is_control() => {
                result.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => result.push(c),
        }
    }
    result.push('"');
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_null() {
        let mut parser = JsonParser::new("null");
        assert_eq!(parser.parse().unwrap(), Value::Nil);
    }

    #[test]
    fn test_parse_booleans() {
        let mut parser = JsonParser::new("true");
        assert_eq!(parser.parse().unwrap(), Value::Bool(true));

        let mut parser = JsonParser::new("false");
        assert_eq!(parser.parse().unwrap(), Value::Bool(false));
    }

    #[test]
    fn test_parse_integers() {
        let mut parser = JsonParser::new("0");
        assert_eq!(parser.parse().unwrap(), Value::Int(0));

        let mut parser = JsonParser::new("42");
        assert_eq!(parser.parse().unwrap(), Value::Int(42));

        let mut parser = JsonParser::new("-17");
        assert_eq!(parser.parse().unwrap(), Value::Int(-17));

        let mut parser = JsonParser::new("9223372036854775807");
        assert_eq!(parser.parse().unwrap(), Value::Int(9223372036854775807));
    }

    #[test]
    #[allow(clippy::approx_constant)]
    fn test_parse_floats() {
        let mut parser = JsonParser::new("3.14");
        match parser.parse().unwrap() {
            Value::Float(f) => assert!((f - 3.14).abs() < 1e-10),
            _ => panic!("Expected float"),
        }

        let mut parser = JsonParser::new("-0.5");
        match parser.parse().unwrap() {
            Value::Float(f) => assert!((f - (-0.5)).abs() < 1e-10),
            _ => panic!("Expected float"),
        }

        let mut parser = JsonParser::new("1e10");
        match parser.parse().unwrap() {
            Value::Float(f) => assert!((f - 1e10).abs() < 1e5),
            _ => panic!("Expected float"),
        }

        let mut parser = JsonParser::new("2.5e-3");
        match parser.parse().unwrap() {
            Value::Float(f) => assert!((f - 0.0025).abs() < 1e-10),
            _ => panic!("Expected float"),
        }

        let mut parser = JsonParser::new("1.0");
        match parser.parse().unwrap() {
            Value::Float(f) => assert!((f - 1.0).abs() < 1e-10),
            _ => panic!("Expected float"),
        }
    }

    #[test]
    fn test_parse_strings() {
        let mut parser = JsonParser::new("\"hello\"");
        assert_eq!(parser.parse().unwrap(), Value::String(Rc::from("hello")));

        let mut parser = JsonParser::new("\"\"");
        assert_eq!(parser.parse().unwrap(), Value::String(Rc::from("")));

        let mut parser = JsonParser::new("\"hello\\nworld\"");
        assert_eq!(
            parser.parse().unwrap(),
            Value::String(Rc::from("hello\nworld"))
        );

        let mut parser = JsonParser::new("\"quote\\\"test\"");
        assert_eq!(
            parser.parse().unwrap(),
            Value::String(Rc::from("quote\"test"))
        );

        let mut parser = JsonParser::new("\"backslash\\\\test\"");
        assert_eq!(
            parser.parse().unwrap(),
            Value::String(Rc::from("backslash\\test"))
        );

        let mut parser = JsonParser::new("\"tab\\there\"");
        assert_eq!(
            parser.parse().unwrap(),
            Value::String(Rc::from("tab\there"))
        );

        let mut parser = JsonParser::new("\"\\u0041\"");
        assert_eq!(parser.parse().unwrap(), Value::String(Rc::from("A")));
    }

    #[test]
    fn test_parse_arrays() {
        let mut parser = JsonParser::new("[]");
        assert_eq!(parser.parse().unwrap(), Value::Nil);

        let mut parser = JsonParser::new("[1,2,3]");
        let result = parser.parse().unwrap();
        let vec = result.list_to_vec().unwrap();
        assert_eq!(vec.len(), 3);
        assert_eq!(vec[0], Value::Int(1));
        assert_eq!(vec[1], Value::Int(2));
        assert_eq!(vec[2], Value::Int(3));

        let mut parser = JsonParser::new("[1,\"two\",true,null]");
        let result = parser.parse().unwrap();
        let vec = result.list_to_vec().unwrap();
        assert_eq!(vec.len(), 4);
        assert_eq!(vec[0], Value::Int(1));
        assert_eq!(vec[1], Value::String(Rc::from("two")));
        assert_eq!(vec[2], Value::Bool(true));
        assert_eq!(vec[3], Value::Nil);
    }

    #[test]
    fn test_parse_objects() {
        let mut parser = JsonParser::new("{}");
        match parser.parse().unwrap() {
            Value::Table(t) => {
                assert_eq!(t.borrow().len(), 0);
            }
            _ => panic!("Expected table"),
        }

        let mut parser = JsonParser::new("{\"name\":\"Alice\",\"age\":30}");
        match parser.parse().unwrap() {
            Value::Table(t) => {
                let table = t.borrow();
                assert_eq!(table.len(), 2);
                assert_eq!(
                    table.get(&TableKey::String("name".to_string())),
                    Some(&Value::String(Rc::from("Alice")))
                );
                assert_eq!(
                    table.get(&TableKey::String("age".to_string())),
                    Some(&Value::Int(30))
                );
            }
            _ => panic!("Expected table"),
        }
    }

    #[test]
    fn test_parse_whitespace() {
        let mut parser = JsonParser::new("  \n\t  42  \n\t  ");
        assert_eq!(parser.parse().unwrap(), Value::Int(42));

        let mut parser = JsonParser::new("[ 1 , 2 , 3 ]");
        let result = parser.parse().unwrap();
        let vec = result.list_to_vec().unwrap();
        assert_eq!(vec.len(), 3);
    }

    #[test]
    fn test_parse_errors() {
        let mut parser = JsonParser::new("");
        assert!(parser.parse().is_err());

        let mut parser = JsonParser::new("42 extra");
        assert!(parser.parse().is_err());

        let mut parser = JsonParser::new("\"unterminated");
        assert!(parser.parse().is_err());

        let mut parser = JsonParser::new("[1,2");
        assert!(parser.parse().is_err());

        let mut parser = JsonParser::new("{\"key\":42");
        assert!(parser.parse().is_err());

        let mut parser = JsonParser::new("invalid");
        assert!(parser.parse().is_err());
    }

    #[test]
    fn test_serialize_compact() {
        assert_eq!(serialize_value(&Value::Nil).unwrap(), "null");
        assert_eq!(serialize_value(&Value::Bool(true)).unwrap(), "true");
        assert_eq!(serialize_value(&Value::Bool(false)).unwrap(), "false");
        assert_eq!(serialize_value(&Value::Int(42)).unwrap(), "42");
        assert_eq!(serialize_value(&Value::Int(-17)).unwrap(), "-17");

        #[allow(clippy::approx_constant)]
        {
            match serialize_value(&Value::Float(3.14)) {
                Ok(s) => assert!(s.contains("3.14")),
                Err(e) => panic!("Error: {}", e),
            }
        }

        assert_eq!(
            serialize_value(&Value::String(Rc::from("hello"))).unwrap(),
            "\"hello\""
        );

        let list = list(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
        assert_eq!(serialize_value(&list).unwrap(), "[1,2,3]");

        let mut map = BTreeMap::new();
        map.insert(
            TableKey::String("name".to_string()),
            Value::String(Rc::from("Alice")),
        );
        map.insert(TableKey::String("age".to_string()), Value::Int(30));
        let table = Value::Table(Rc::new(std::cell::RefCell::new(map)));
        let serialized = serialize_value(&table).unwrap();
        assert!(serialized.contains("\"name\":\"Alice\""));
        assert!(serialized.contains("\"age\":30"));
    }

    #[test]
    fn test_serialize_string_escaping() {
        assert_eq!(
            serialize_value(&Value::String(Rc::from("hello\"world"))).unwrap(),
            "\"hello\\\"world\""
        );

        assert_eq!(
            serialize_value(&Value::String(Rc::from("hello\\world"))).unwrap(),
            "\"hello\\\\world\""
        );

        assert_eq!(
            serialize_value(&Value::String(Rc::from("hello\nworld"))).unwrap(),
            "\"hello\\nworld\""
        );

        assert_eq!(
            serialize_value(&Value::String(Rc::from("hello\tworld"))).unwrap(),
            "\"hello\\tworld\""
        );
    }

    #[test]
    fn test_serialize_roundtrip() {
        let original = list(vec![
            Value::Int(1),
            Value::String(Rc::from("test")),
            Value::Bool(true),
            Value::Nil,
        ]);

        let serialized = serialize_value(&original).unwrap();
        let mut parser = JsonParser::new(&serialized);
        let deserialized = parser.parse().unwrap();

        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_serialize_pretty() {
        let list = list(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
        let pretty = serialize_value_pretty(&list, 0).unwrap();
        assert!(pretty.contains('\n'));
        assert!(pretty.contains("  "));

        let mut map = BTreeMap::new();
        map.insert(TableKey::String("key".to_string()), Value::Int(42));
        let table = Value::Table(Rc::new(std::cell::RefCell::new(map)));
        let pretty = serialize_value_pretty(&table, 0).unwrap();
        assert!(pretty.contains('\n'));
        assert!(pretty.contains("  "));
    }

    #[test]
    fn test_serialize_errors() {
        let closure = Value::Closure(Rc::new(crate::value::Closure {
            bytecode: Rc::new(vec![]),
            arity: crate::value::Arity::Exact(0),
            env: Rc::new(vec![]),
            num_locals: 0,
            num_captures: 0,
            constants: Rc::new(vec![]),
        }));
        assert!(serialize_value(&closure).is_err());

        let native_fn: crate::value::NativeFn = |_| Ok(Value::Nil);
        let fn_val = Value::NativeFn(native_fn);
        assert!(serialize_value(&fn_val).is_err());
    }

    #[test]
    fn test_float_formatting() {
        match serialize_value(&Value::Float(1.0)) {
            Ok(s) => assert!(
                s.contains("."),
                "Float 1.0 should contain decimal point, got: {}",
                s
            ),
            Err(e) => panic!("Error: {}", e),
        }

        match serialize_value(&Value::Float(42.0)) {
            Ok(s) => assert!(
                s.contains("."),
                "Float 42.0 should contain decimal point, got: {}",
                s
            ),
            Err(e) => panic!("Error: {}", e),
        }
    }

    #[test]
    fn test_parse_leading_zeros() {
        // Leading zeros are not allowed in JSON
        let mut parser = JsonParser::new("01");
        assert!(parser.parse().is_err());

        let mut parser = JsonParser::new("00");
        assert!(parser.parse().is_err());

        // But "0" alone is valid
        let mut parser = JsonParser::new("0");
        assert_eq!(parser.parse().unwrap(), Value::Int(0));

        // And "0.1" is valid
        let mut parser = JsonParser::new("0.1");
        match parser.parse().unwrap() {
            Value::Float(f) => assert!((f - 0.1).abs() < 1e-10),
            _ => panic!("Expected float"),
        }
    }

    #[test]
    fn test_parse_trailing_comma() {
        // Trailing commas are not allowed in JSON
        let mut parser = JsonParser::new("[1,2,]");
        assert!(parser.parse().is_err());

        let mut parser = JsonParser::new("{\"a\":1,}");
        assert!(parser.parse().is_err());
    }

    #[test]
    fn test_serialize_nan_infinity() {
        // NaN should error
        assert!(serialize_value(&Value::Float(f64::NAN)).is_err());

        // Positive infinity should error
        assert!(serialize_value(&Value::Float(f64::INFINITY)).is_err());

        // Negative infinity should error
        assert!(serialize_value(&Value::Float(f64::NEG_INFINITY)).is_err());
    }

    #[test]
    fn test_serialize_non_string_table_key() {
        let mut map = BTreeMap::new();
        map.insert(TableKey::Int(42), Value::String(Rc::from("value")));
        let table = Value::Table(Rc::new(std::cell::RefCell::new(map)));

        // Should error because key is not a string
        assert!(serialize_value(&table).is_err());
    }

    #[test]
    fn test_json_parse_wrong_type() {
        // json-parse requires a string argument
        let result = prim_json_parse(&[Value::Int(42)]);
        assert!(result.is_err());
    }

    #[test]
    fn test_json_serialize_wrong_arity() {
        // json-serialize requires exactly 1 argument
        let result = prim_json_serialize(&[]);
        assert!(result.is_err());

        let result = prim_json_serialize(&[Value::Int(1), Value::Int(2)]);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_surrogate_pair() {
        // Emoji 😀 is U+1F600, encoded as surrogate pair \uD83D\uDE00
        let mut parser = JsonParser::new("\"\\uD83D\\uDE00\"");
        match parser.parse().unwrap() {
            Value::String(s) => {
                assert_eq!(s.as_ref(), "😀");
            }
            _ => panic!("Expected string"),
        }
    }

    #[test]
    fn test_parse_lone_surrogate() {
        // High surrogate without low surrogate should error
        let mut parser = JsonParser::new("\"\\uD800\"");
        assert!(parser.parse().is_err());

        // Low surrogate without high surrogate should error
        let mut parser = JsonParser::new("\"\\uDC00\"");
        assert!(parser.parse().is_err());
    }
}
