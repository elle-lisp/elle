use crate::value::{Condition, Value};

/// Display values to standard output
pub fn prim_display(args: &[Value]) -> Result<Value, Condition> {
    for arg in args {
        print!("{}", format_value(arg));
    }
    Ok(Value::NIL)
}

/// Print values followed by a newline (Common Lisp-style print)
pub fn prim_print(args: &[Value]) -> Result<Value, Condition> {
    for arg in args {
        print!("{}", format_value(arg));
    }
    println!();
    Ok(Value::NIL)
}

/// Format a value for display, using the symbol table if available
fn format_value(value: &Value) -> String {
    if let Some(sym_id) = value.as_symbol() {
        // Try to get the symbol name from the thread-local symbol table
        unsafe {
            if let Some(symbols_ptr) = crate::ffi::primitives::context::get_symbol_table() {
                if let Some(name) = (*symbols_ptr).name(crate::value::SymbolId(sym_id)) {
                    return name.to_string();
                }
            }
        }
        // Fallback if symbol table is not available
        format!("Symbol({})", sym_id)
    } else if let Some(id) = value.as_keyword() {
        // Try to get the keyword name from the thread-local symbol table
        unsafe {
            if let Some(symbols_ptr) = crate::ffi::primitives::context::get_symbol_table() {
                if let Some(name) = (*symbols_ptr).name(crate::value::SymbolId(id)) {
                    return format!(":{}", name);
                }
            }
        }
        // Fallback if symbol table is not available
        format!(":keyword-{}", id)
    } else {
        // Use the Debug implementation for Value
        format!("{:?}", value)
    }
}

/// Print a newline
pub fn prim_newline(_args: &[Value]) -> Result<Value, Condition> {
    println!();
    Ok(Value::NIL)
}
