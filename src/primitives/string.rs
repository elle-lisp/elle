//! String manipulation primitives
use crate::value::Value;
use std::rc::Rc;

/// Get the length of a string
pub fn prim_string_length(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("string-length requires exactly 1 argument".to_string());
    }
    match &args[0] {
        Value::String(s) => Ok(Value::Int(s.chars().count() as i64)),
        _ => Err("string-length requires a string".to_string()),
    }
}

/// Append multiple strings
pub fn prim_string_append(args: &[Value]) -> Result<Value, String> {
    let mut result = String::new();
    for arg in args {
        match arg {
            Value::String(s) => result.push_str(s),
            _ => return Err("string-append requires all arguments to be strings".to_string()),
        }
    }
    Ok(Value::String(Rc::from(result)))
}

/// Convert string to uppercase
pub fn prim_string_upcase(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("string-upcase requires exactly 1 argument".to_string());
    }
    match &args[0] {
        Value::String(s) => Ok(Value::String(Rc::from(s.to_uppercase()))),
        _ => Err("string-upcase requires a string".to_string()),
    }
}

/// Convert string to lowercase
pub fn prim_string_downcase(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("string-downcase requires exactly 1 argument".to_string());
    }
    match &args[0] {
        Value::String(s) => Ok(Value::String(Rc::from(s.to_lowercase()))),
        _ => Err("string-downcase requires a string".to_string()),
    }
}

/// Get a substring
pub fn prim_substring(args: &[Value]) -> Result<Value, String> {
    if args.len() < 2 || args.len() > 3 {
        return Err("substring requires 2 or 3 arguments (string, start [, end])".to_string());
    }

    let s = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err("substring requires a string as first argument".to_string()),
    };

    let start = args[1].as_int()? as usize;
    let char_count = s.chars().count();
    let end = if args.len() == 3 {
        args[2].as_int()? as usize
    } else {
        char_count
    };

    if start > char_count || end > char_count || start > end {
        return Err(format!(
            "substring indices out of range: start={}, end={}, length={}",
            start, end, char_count
        ));
    }

    // Convert character indices to byte indices
    let byte_start = s
        .char_indices()
        .nth(start)
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    let byte_end = s.char_indices().nth(end).map(|(i, _)| i).unwrap_or(s.len());
    Ok(Value::String(Rc::from(&s[byte_start..byte_end])))
}

/// Find the index of a character
pub fn prim_string_index(args: &[Value]) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("string-index requires exactly 2 arguments (string, char)".to_string());
    }

    let haystack = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err("string-index requires a string as first argument".to_string()),
    };

    let needle = match &args[1] {
        Value::String(s) => {
            if s.chars().count() != 1 {
                return Err(
                    "string-index requires a single character as second argument".to_string(),
                );
            }
            s.chars().next().unwrap()
        }
        _ => return Err("string-index requires a string as second argument".to_string()),
    };

    match haystack.chars().position(|ch| ch == needle) {
        Some(pos) => Ok(Value::Int(pos as i64)),
        None => Ok(Value::Nil),
    }
}

/// Get a character at an index
pub fn prim_char_at(args: &[Value]) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("char-at requires exactly 2 arguments (string, index)".to_string());
    }

    let s = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err("char-at requires a string as first argument".to_string()),
    };

    let index = args[1].as_int()? as usize;
    let char_count = s.chars().count();

    if index >= char_count {
        return Err(format!(
            "Index out of bounds: index={}, length={}",
            index, char_count
        ));
    }

    match s.chars().nth(index) {
        Some(c) => Ok(Value::String(Rc::from(c.to_string()))),
        None => Err(format!(
            "Index out of bounds: index={}, length={}",
            index, char_count
        )),
    }
}

/// Convert to integer
pub fn prim_to_int(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("int requires exactly 1 argument".to_string());
    }
    match &args[0] {
        Value::Int(n) => Ok(Value::Int(*n)),
        Value::Float(f) => Ok(Value::Int(*f as i64)),
        Value::String(s) => s
            .parse::<i64>()
            .map(Value::Int)
            .map_err(|_| "Cannot parse string as integer".to_string()),
        _ => Err("Cannot convert to integer".to_string()),
    }
}

/// Convert to float
pub fn prim_to_float(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("float requires exactly 1 argument".to_string());
    }
    match &args[0] {
        Value::Int(n) => Ok(Value::Float(*n as f64)),
        Value::Float(f) => Ok(Value::Float(*f)),
        Value::String(s) => s
            .parse::<f64>()
            .map(Value::Float)
            .map_err(|_| "Cannot parse string as float".to_string()),
        _ => Err("Cannot convert to float".to_string()),
    }
}

/// Convert to string
pub fn prim_to_string(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("string requires exactly 1 argument".to_string());
    }
    Ok(Value::String(Rc::from(args[0].to_string())))
}

/// Split string on delimiter
pub fn prim_string_split(args: &[Value]) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("string-split requires exactly 2 arguments".to_string());
    }

    let s = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err("string-split requires a string as first argument".to_string()),
    };

    let delimiter = match &args[1] {
        Value::String(d) => d.as_ref(),
        _ => return Err("string-split requires a string as second argument".to_string()),
    };

    if delimiter.is_empty() {
        return Err("string-split delimiter cannot be empty".to_string());
    }

    let parts: Vec<Value> = s
        .split(delimiter)
        .map(|part| Value::String(Rc::from(part)))
        .collect();

    Ok(crate::value::list(parts))
}

/// Replace all occurrences of old with new
pub fn prim_string_replace(args: &[Value]) -> Result<Value, String> {
    if args.len() != 3 {
        return Err("string-replace requires exactly 3 arguments".to_string());
    }

    let s = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err("string-replace requires a string as first argument".to_string()),
    };

    let old = match &args[1] {
        Value::String(o) => o.as_ref(),
        _ => return Err("string-replace requires a string as second argument".to_string()),
    };

    if old.is_empty() {
        return Err("string-replace search string cannot be empty".to_string());
    }

    let new = match &args[2] {
        Value::String(n) => n.as_ref(),
        _ => return Err("string-replace requires a string as third argument".to_string()),
    };

    Ok(Value::String(Rc::from(s.replace(old, new))))
}

/// Trim leading and trailing whitespace
pub fn prim_string_trim(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("string-trim requires exactly 1 argument".to_string());
    }

    match &args[0] {
        Value::String(s) => Ok(Value::String(Rc::from(s.trim()))),
        _ => Err("string-trim requires a string".to_string()),
    }
}

/// Check if string contains substring
pub fn prim_string_contains(args: &[Value]) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("string-contains? requires exactly 2 arguments".to_string());
    }

    let haystack = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err("string-contains? requires a string as first argument".to_string()),
    };

    let needle = match &args[1] {
        Value::String(n) => n.as_ref(),
        _ => return Err("string-contains? requires a string as second argument".to_string()),
    };

    Ok(Value::Bool(haystack.contains(needle)))
}

/// Check if string starts with prefix
pub fn prim_string_starts_with(args: &[Value]) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("string-starts-with? requires exactly 2 arguments".to_string());
    }

    let s = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err("string-starts-with? requires a string as first argument".to_string()),
    };

    let prefix = match &args[1] {
        Value::String(p) => p.as_ref(),
        _ => return Err("string-starts-with? requires a string as second argument".to_string()),
    };

    Ok(Value::Bool(s.starts_with(prefix)))
}

/// Check if string ends with suffix
pub fn prim_string_ends_with(args: &[Value]) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("string-ends-with? requires exactly 2 arguments".to_string());
    }

    let s = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err("string-ends-with? requires a string as first argument".to_string()),
    };

    let suffix = match &args[1] {
        Value::String(suf) => suf.as_ref(),
        _ => return Err("string-ends-with? requires a string as second argument".to_string()),
    };

    Ok(Value::Bool(s.ends_with(suffix)))
}

/// Join list of strings with separator
pub fn prim_string_join(args: &[Value]) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("string-join requires exactly 2 arguments".to_string());
    }

    let list = &args[0];
    let separator = match &args[1] {
        Value::String(s) => s.as_ref(),
        _ => return Err("string-join requires a string as second argument".to_string()),
    };

    let vec = list.list_to_vec()?;
    let mut strings = Vec::new();

    for val in vec {
        match val {
            Value::String(s) => strings.push(s.to_string()),
            _ => return Err("string-join requires a list of strings".to_string()),
        }
    }

    Ok(Value::String(Rc::from(strings.join(separator))))
}

/// Convert number to string
pub fn prim_number_to_string(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("number->string requires exactly 1 argument".to_string());
    }

    match &args[0] {
        Value::Int(n) => Ok(Value::String(Rc::from(n.to_string()))),
        Value::Float(f) => Ok(Value::String(Rc::from(f.to_string()))),
        _ => Err("number->string requires a number".to_string()),
    }
}
