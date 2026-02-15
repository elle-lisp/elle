//! String manipulation primitives
use crate::error::{LError, LResult};
use crate::value::Value;
use crate::vm::VM;
use std::rc::Rc;

/// Get the length of a string
pub fn prim_string_length(args: &[Value]) -> LResult<Value> {
    if args.len() != 1 {
        return Err(LError::arity_mismatch(1, args.len()));
    }
    match &args[0] {
        Value::String(s) => Ok(Value::Int(s.chars().count() as i64)),
        _ => Err(LError::type_mismatch("string", args[0].type_name())),
    }
}

/// Append multiple strings
pub fn prim_string_append(args: &[Value]) -> LResult<Value> {
    let mut result = String::new();
    for arg in args {
        match arg {
            Value::String(s) => result.push_str(s),
            _ => return Err(LError::type_mismatch("string", arg.type_name())),
        }
    }
    Ok(Value::String(Rc::from(result)))
}

/// Convert string to uppercase
pub fn prim_string_upcase(args: &[Value]) -> LResult<Value> {
    if args.len() != 1 {
        return Err(LError::arity_mismatch(1, args.len()));
    }
    match &args[0] {
        Value::String(s) => Ok(Value::String(Rc::from(s.to_uppercase()))),
        _ => Err(LError::type_mismatch("string", args[0].type_name())),
    }
}

/// Convert string to lowercase
pub fn prim_string_downcase(args: &[Value]) -> LResult<Value> {
    if args.len() != 1 {
        return Err(LError::arity_mismatch(1, args.len()));
    }
    match &args[0] {
        Value::String(s) => Ok(Value::String(Rc::from(s.to_lowercase()))),
        _ => Err(LError::type_mismatch("string", args[0].type_name())),
    }
}

/// Get a substring
pub fn prim_substring(args: &[Value]) -> LResult<Value> {
    if args.len() < 2 || args.len() > 3 {
        return Err(LError::arity_range(2, 3, args.len()));
    }

    let s = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err(LError::type_mismatch("string", args[0].type_name())),
    };

    let start = args[1].as_int()? as usize;
    let char_count = s.chars().count();
    let end = if args.len() == 3 {
        args[2].as_int()? as usize
    } else {
        char_count
    };

    if start > char_count || end > char_count || start > end {
        return Err(LError::runtime_error(format!(
            "substring indices out of range: start={}, end={}, length={}",
            start, end, char_count
        )));
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
pub fn prim_string_index(args: &[Value]) -> LResult<Value> {
    if args.len() != 2 {
        return Err(LError::arity_mismatch(2, args.len()));
    }

    let haystack = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err(LError::type_mismatch("string", args[0].type_name())),
    };

    let needle = match &args[1] {
        Value::String(s) => {
            if s.chars().count() != 1 {
                return Err(LError::argument_error(
                    "string-index requires a single character as second argument",
                ));
            }
            s.chars().next().unwrap()
        }
        _ => return Err(LError::type_mismatch("string", args[1].type_name())),
    };

    match haystack.chars().position(|ch| ch == needle) {
        Some(pos) => Ok(Value::Int(pos as i64)),
        None => Ok(Value::Nil),
    }
}

/// Get a character at an index
pub fn prim_char_at(args: &[Value]) -> LResult<Value> {
    if args.len() != 2 {
        return Err(LError::arity_mismatch(2, args.len()));
    }

    let s = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err(LError::type_mismatch("string", args[0].type_name())),
    };

    let index = args[1].as_int()? as usize;
    let char_count = s.chars().count();

    if index >= char_count {
        return Err(LError::index_out_of_bounds(index as isize, char_count));
    }

    match s.chars().nth(index) {
        Some(c) => Ok(Value::String(Rc::from(c.to_string()))),
        None => Err(LError::index_out_of_bounds(index as isize, char_count)),
    }
}

/// Convert to integer
pub fn prim_to_int(args: &[Value]) -> LResult<Value> {
    if args.len() != 1 {
        return Err(LError::arity_mismatch(1, args.len()));
    }
    match &args[0] {
        Value::Int(n) => Ok(Value::Int(*n)),
        Value::Float(f) => Ok(Value::Int(*f as i64)),
        Value::String(s) => s
            .parse::<i64>()
            .map(Value::Int)
            .map_err(|_| LError::argument_error("Cannot parse string as integer")),
        _ => Err(LError::type_mismatch(
            "integer-convertible",
            args[0].type_name(),
        )),
    }
}

/// Convert to float
pub fn prim_to_float(args: &[Value]) -> LResult<Value> {
    if args.len() != 1 {
        return Err(LError::arity_mismatch(1, args.len()));
    }
    match &args[0] {
        Value::Int(n) => Ok(Value::Float(*n as f64)),
        Value::Float(f) => Ok(Value::Float(*f)),
        Value::String(s) => s
            .parse::<f64>()
            .map(Value::Float)
            .map_err(|_| LError::argument_error("Cannot parse string as float")),
        _ => Err(LError::type_mismatch(
            "float-convertible",
            args[0].type_name(),
        )),
    }
}

/// Convert to string
pub fn prim_to_string(args: &[Value]) -> LResult<Value> {
    if args.len() != 1 {
        return Err(LError::arity_mismatch(1, args.len()));
    }
    Ok(Value::String(Rc::from(args[0].to_string())))
}

/// Split string on delimiter
pub fn prim_string_split(args: &[Value]) -> LResult<Value> {
    if args.len() != 2 {
        return Err(LError::arity_mismatch(2, args.len()));
    }

    let s = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err(LError::type_mismatch("string", args[0].type_name())),
    };

    let delimiter = match &args[1] {
        Value::String(d) => d.as_ref(),
        _ => return Err(LError::type_mismatch("string", args[1].type_name())),
    };

    if delimiter.is_empty() {
        return Err(LError::argument_error(
            "string-split delimiter cannot be empty",
        ));
    }

    let parts: Vec<Value> = s
        .split(delimiter)
        .map(|part| Value::String(Rc::from(part)))
        .collect();

    Ok(crate::value::list(parts))
}

/// Replace all occurrences of old with new
pub fn prim_string_replace(args: &[Value]) -> LResult<Value> {
    if args.len() != 3 {
        return Err(LError::arity_mismatch(3, args.len()));
    }

    let s = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err(LError::type_mismatch("string", args[0].type_name())),
    };

    let old = match &args[1] {
        Value::String(o) => o.as_ref(),
        _ => return Err(LError::type_mismatch("string", args[1].type_name())),
    };

    if old.is_empty() {
        return Err(LError::argument_error(
            "string-replace search string cannot be empty",
        ));
    }

    let new = match &args[2] {
        Value::String(n) => n.as_ref(),
        _ => return Err(LError::type_mismatch("string", args[2].type_name())),
    };

    Ok(Value::String(Rc::from(s.replace(old, new))))
}

/// Trim leading and trailing whitespace
pub fn prim_string_trim(args: &[Value]) -> LResult<Value> {
    if args.len() != 1 {
        return Err(LError::arity_mismatch(1, args.len()));
    }

    match &args[0] {
        Value::String(s) => Ok(Value::String(Rc::from(s.trim()))),
        _ => Err(LError::type_mismatch("string", args[0].type_name())),
    }
}

/// Check if string contains substring
pub fn prim_string_contains(args: &[Value]) -> LResult<Value> {
    if args.len() != 2 {
        return Err(LError::arity_mismatch(2, args.len()));
    }

    let haystack = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err(LError::type_mismatch("string", args[0].type_name())),
    };

    let needle = match &args[1] {
        Value::String(n) => n.as_ref(),
        _ => return Err(LError::type_mismatch("string", args[1].type_name())),
    };

    Ok(Value::Bool(haystack.contains(needle)))
}

/// Check if string starts with prefix
pub fn prim_string_starts_with(args: &[Value]) -> LResult<Value> {
    if args.len() != 2 {
        return Err(LError::arity_mismatch(2, args.len()));
    }

    let s = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err(LError::type_mismatch("string", args[0].type_name())),
    };

    let prefix = match &args[1] {
        Value::String(p) => p.as_ref(),
        _ => return Err(LError::type_mismatch("string", args[1].type_name())),
    };

    Ok(Value::Bool(s.starts_with(prefix)))
}

/// Check if string ends with suffix
pub fn prim_string_ends_with(args: &[Value]) -> LResult<Value> {
    if args.len() != 2 {
        return Err(LError::arity_mismatch(2, args.len()));
    }

    let s = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err(LError::type_mismatch("string", args[0].type_name())),
    };

    let suffix = match &args[1] {
        Value::String(suf) => suf.as_ref(),
        _ => return Err(LError::type_mismatch("string", args[1].type_name())),
    };

    Ok(Value::Bool(s.ends_with(suffix)))
}

/// Join list of strings with separator
pub fn prim_string_join(args: &[Value]) -> LResult<Value> {
    if args.len() != 2 {
        return Err(LError::arity_mismatch(2, args.len()));
    }

    let list = &args[0];
    let separator = match &args[1] {
        Value::String(s) => s.as_ref(),
        _ => return Err(LError::type_mismatch("string", args[1].type_name())),
    };

    let vec = list.list_to_vec()?;
    let mut strings = Vec::new();

    for val in vec {
        match val {
            Value::String(s) => strings.push(s.to_string()),
            _ => return Err(LError::type_mismatch("string", val.type_name())),
        }
    }

    Ok(Value::String(Rc::from(strings.join(separator))))
}

/// Convert number to string
pub fn prim_number_to_string(args: &[Value]) -> LResult<Value> {
    if args.len() != 1 {
        return Err(LError::arity_mismatch(1, args.len()));
    }

    match &args[0] {
        Value::Int(n) => Ok(Value::String(Rc::from(n.to_string()))),
        Value::Float(f) => Ok(Value::String(Rc::from(f.to_string()))),
        _ => Err(LError::type_mismatch("number", args[0].type_name())),
    }
}

// ============ SCHEME-STYLE CONVERSION ALIASES ============

/// Convert string to integer (Scheme-style name)
/// `(string->int str)`
pub fn prim_string_to_int(args: &[Value]) -> LResult<Value> {
    prim_to_int(args)
}

/// Convert string to float (Scheme-style name)
/// `(string->float str)`
pub fn prim_string_to_float(args: &[Value]) -> LResult<Value> {
    prim_to_float(args)
}

/// Convert any value to string (Scheme-style name)
/// `(any->string val)`
pub fn prim_any_to_string(args: &[Value]) -> LResult<Value> {
    prim_to_string(args)
}

/// Convert symbol to string
/// `(symbol->string sym)`
pub fn prim_symbol_to_string(args: &[Value], _vm: &mut VM) -> LResult<Value> {
    if args.len() != 1 {
        return Err(LError::arity_mismatch(1, args.len()));
    }

    match &args[0] {
        Value::Symbol(id) => {
            // SAFETY: The symbol table is set in main.rs before any code execution
            unsafe {
                if let Some(symbols_ptr) = crate::ffi::primitives::context::get_symbol_table() {
                    let symbols = &*symbols_ptr;
                    if let Some(name) = symbols.name(*id) {
                        Ok(Value::String(Rc::from(name)))
                    } else {
                        Err(LError::runtime_error(format!(
                            "Symbol ID {} not found in symbol table",
                            id.0
                        )))
                    }
                } else {
                    Err(LError::runtime_error("Symbol table not available"))
                }
            }
        }
        _ => Err(LError::type_mismatch("symbol", args[0].type_name())),
    }
}
