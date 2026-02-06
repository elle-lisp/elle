//! Type checking primitives
use crate::ffi::primitives::context::get_symbol_table;
use crate::value::Value;

/// Check if value is nil
pub fn prim_is_nil(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("nil? requires exactly 1 argument".to_string());
    }
    Ok(Value::Bool(args[0].is_nil()))
}

/// Check if value is a pair (cons cell)
pub fn prim_is_pair(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("pair? requires exactly 1 argument".to_string());
    }
    Ok(Value::Bool(matches!(args[0], Value::Cons(_))))
}

/// Check if value is a number
pub fn prim_is_number(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("number? requires exactly 1 argument".to_string());
    }
    Ok(Value::Bool(matches!(
        args[0],
        Value::Int(_) | Value::Float(_)
    )))
}

/// Check if value is a symbol
pub fn prim_is_symbol(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("symbol? requires exactly 1 argument".to_string());
    }
    Ok(Value::Bool(matches!(args[0], Value::Symbol(_))))
}

/// Check if value is a string
pub fn prim_is_string(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("string? requires exactly 1 argument".to_string());
    }
    Ok(Value::Bool(matches!(args[0], Value::String(_))))
}

/// Check if value is a boolean
pub fn prim_is_bool(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("bool? requires exactly 1 argument".to_string());
    }
    Ok(Value::Bool(matches!(args[0], Value::Bool(_))))
}

/// Get the type name of a value as a keyword
pub fn prim_type(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("type requires exactly 1 argument".to_string());
    }

    let type_name = args[0].type_name();

    // Try to get the symbol table from thread-local context
    // Safety: The symbol table pointer is set in main() and cleared only at exit,
    // so it's valid during program execution.
    unsafe {
        if let Some(symbols_ptr) = get_symbol_table() {
            let keyword_id = (*symbols_ptr).intern(type_name);
            Ok(Value::Keyword(keyword_id))
        } else {
            // Fallback to string if no symbol table in context
            Ok(Value::String(std::rc::Rc::from(type_name.to_string())))
        }
    }
}
