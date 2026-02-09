use crate::value::{TableKey, Value};

/// Escape a string for JSON output
pub fn escape_json_string(s: &str) -> String {
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

/// Serialize a value to compact JSON
pub fn serialize_value(value: &Value) -> Result<String, String> {
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
        Value::ThreadHandle(_) => Err("Cannot serialize thread handles to JSON".to_string()),
    }
}

/// Serialize a value to pretty-printed JSON with indentation
pub fn serialize_value_pretty(value: &Value, indent_level: usize) -> Result<String, String> {
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
        Value::ThreadHandle(_) => Err("Cannot serialize thread handles to JSON".to_string()),
    }
}
