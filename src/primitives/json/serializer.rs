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
    if value.is_nil() {
        Ok("null".to_string())
    } else if let Some(b) = value.as_bool() {
        Ok(if b { "true" } else { "false" }.to_string())
    } else if let Some(i) = value.as_int() {
        Ok(i.to_string())
    } else if let Some(f) = value.as_float() {
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
    } else if let Some(s) = value.as_string() {
        Ok(escape_json_string(s))
    } else if value.is_cons() {
        // Convert list to array
        let vec = value.list_to_vec()?;
        let elements: Result<Vec<String>, String> = vec.iter().map(serialize_value).collect();
        Ok(format!("[{}]", elements?.join(",")))
    } else if let Some(v) = value.as_vector() {
        let borrowed = v.borrow();
        let elements: Result<Vec<String>, String> = borrowed.iter().map(serialize_value).collect();
        Ok(format!("[{}]", elements?.join(",")))
    } else if let Some(t) = value.as_table() {
        let table = t.borrow();
        let mut pairs = Vec::new();
        for (k, v) in table.iter() {
            let key_str = match k {
                TableKey::String(s) => escape_json_string(s),
                _ => return Err("Table keys must be strings for JSON serialization".to_string()),
            };
            let val_str = serialize_value(v)?;
            pairs.push(format!("{}:{}", key_str, val_str));
        }
        Ok(format!("{{{}}}", pairs.join(",")))
    } else if let Some(s) = value.as_struct() {
        let mut pairs = Vec::new();
        for (k, v) in s.iter() {
            let key_str = match k {
                TableKey::String(s) => escape_json_string(s),
                _ => return Err("Struct keys must be strings for JSON serialization".to_string()),
            };
            let val_str = serialize_value(v)?;
            pairs.push(format!("{}:{}", key_str, val_str));
        }
        Ok(format!("{{{}}}", pairs.join(",")))
    } else if value.is_keyword() {
        // Serialize keywords as strings (without the colon prefix)
        // Note: We don't have access to the symbol table here, so we'll use the ID
        // In practice, keywords should be converted to strings before serialization
        Err("Cannot serialize keyword without symbol table context".to_string())
    } else if value.is_closure() {
        Err("Cannot serialize closures to JSON".to_string())
    } else if value.is_symbol() {
        Err("Cannot serialize symbols to JSON".to_string())
    } else if let Some(cell) = value.as_cell() {
        // Dereference the cell and serialize the inner value
        let inner = cell.borrow();
        serialize_value(&inner)
    } else if let Some(tag) = value.heap_tag() {
        use crate::value::heap::HeapTag;
        match tag {
            HeapTag::String => Err("String should have been handled above".to_string()),
            HeapTag::Cons => Err("Cons should have been handled above".to_string()),
            HeapTag::Vector => Err("Vector should have been handled above".to_string()),
            HeapTag::Table => Err("Table should have been handled above".to_string()),
            HeapTag::Struct => Err("Struct should have been handled above".to_string()),
            HeapTag::Closure => Err("Cannot serialize closures to JSON".to_string()),
            HeapTag::Condition => Err("Cannot serialize conditions to JSON".to_string()),
            HeapTag::Coroutine => Err("Cannot serialize coroutines to JSON".to_string()),
            HeapTag::Cell => Err("Cell should have been handled above".to_string()),
            HeapTag::Float => {
                // This is a heap-allocated float (for NaN values)
                Err("Cannot serialize non-finite float value to JSON".to_string())
            }
            HeapTag::NativeFn => Err("Cannot serialize native functions to JSON".to_string()),
            HeapTag::VmAwareFn => Err("Cannot serialize VM-aware functions to JSON".to_string()),
            HeapTag::LibHandle => Err("Cannot serialize library handles to JSON".to_string()),
            HeapTag::CHandle => Err("Cannot serialize C handles to JSON".to_string()),
            HeapTag::ThreadHandle => Err("Cannot serialize thread handles to JSON".to_string()),
        }
    } else {
        Err("Cannot serialize unknown value type to JSON".to_string())
    }
}

/// Serialize a value to pretty-printed JSON with indentation
pub fn serialize_value_pretty(value: &Value, indent_level: usize) -> Result<String, String> {
    let indent = "  ".repeat(indent_level);
    let next_indent = "  ".repeat(indent_level + 1);

    if value.is_nil() {
        Ok("null".to_string())
    } else if let Some(b) = value.as_bool() {
        Ok(if b { "true" } else { "false" }.to_string())
    } else if let Some(i) = value.as_int() {
        Ok(i.to_string())
    } else if let Some(f) = value.as_float() {
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
    } else if let Some(s) = value.as_string() {
        Ok(escape_json_string(s))
    } else if value.is_cons() {
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
    } else if let Some(v) = value.as_vector() {
        let borrowed = v.borrow();
        if borrowed.is_empty() {
            return Ok("[]".to_string());
        }
        let elements: Result<Vec<String>, String> = borrowed
            .iter()
            .map(|val| serialize_value_pretty(val, indent_level + 1))
            .collect();
        Ok(format!(
            "[\n{}{}\n{}]",
            next_indent,
            elements?.join(&format!(",\n{}", next_indent)),
            indent
        ))
    } else if let Some(t) = value.as_table() {
        let table = t.borrow();
        if table.is_empty() {
            return Ok("{}".to_string());
        }
        let mut pairs = Vec::new();
        for (k, v) in table.iter() {
            let key_str = match k {
                TableKey::String(s) => escape_json_string(s),
                _ => return Err("Table keys must be strings for JSON serialization".to_string()),
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
    } else if let Some(s) = value.as_struct() {
        if s.is_empty() {
            return Ok("{}".to_string());
        }
        let mut pairs = Vec::new();
        for (k, v) in s.iter() {
            let key_str = match k {
                TableKey::String(s) => escape_json_string(s),
                _ => return Err("Struct keys must be strings for JSON serialization".to_string()),
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
    } else if value.is_keyword() {
        Err("Cannot serialize keyword without symbol table context".to_string())
    } else if value.is_closure() {
        Err("Cannot serialize closures to JSON".to_string())
    } else if value.is_symbol() {
        Err("Cannot serialize symbols to JSON".to_string())
    } else if let Some(cell) = value.as_cell() {
        // Dereference the cell and serialize the inner value
        let inner = cell.borrow();
        serialize_value_pretty(&inner, indent_level)
    } else if let Some(tag) = value.heap_tag() {
        use crate::value::heap::HeapTag;
        match tag {
            HeapTag::String => Err("String should have been handled above".to_string()),
            HeapTag::Cons => Err("Cons should have been handled above".to_string()),
            HeapTag::Vector => Err("Vector should have been handled above".to_string()),
            HeapTag::Table => Err("Table should have been handled above".to_string()),
            HeapTag::Struct => Err("Struct should have been handled above".to_string()),
            HeapTag::Closure => Err("Cannot serialize closures to JSON".to_string()),
            HeapTag::Condition => Err("Cannot serialize conditions to JSON".to_string()),
            HeapTag::Coroutine => Err("Cannot serialize coroutines to JSON".to_string()),
            HeapTag::Cell => Err("Cell should have been handled above".to_string()),
            HeapTag::Float => {
                // This is a heap-allocated float (for NaN values)
                Err("Cannot serialize non-finite float value to JSON".to_string())
            }
            HeapTag::NativeFn => Err("Cannot serialize native functions to JSON".to_string()),
            HeapTag::VmAwareFn => Err("Cannot serialize VM-aware functions to JSON".to_string()),
            HeapTag::LibHandle => Err("Cannot serialize library handles to JSON".to_string()),
            HeapTag::CHandle => Err("Cannot serialize C handles to JSON".to_string()),
            HeapTag::ThreadHandle => Err("Cannot serialize thread handles to JSON".to_string()),
        }
    } else {
        Err("Cannot serialize unknown value type to JSON".to_string())
    }
}
