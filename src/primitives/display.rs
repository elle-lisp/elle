use crate::value::Value;

/// Display values to standard output
pub fn prim_display(args: &[Value]) -> Result<Value, String> {
    for arg in args {
        print!("{}", format_value(arg));
    }
    Ok(Value::Nil)
}

/// Format a value for display, using the symbol table if available
fn format_value(value: &Value) -> String {
    match value {
        Value::Symbol(id) => {
            // Try to get the symbol name from the thread-local symbol table
            unsafe {
                if let Some(symbols_ptr) = crate::ffi::primitives::context::get_symbol_table() {
                    if let Some(name) = (*symbols_ptr).name(*id) {
                        return name.to_string();
                    }
                }
            }
            // Fallback if symbol table is not available
            format!("Symbol({})", id.0)
        }
        _ => format!("{}", value),
    }
}

/// Print a newline
pub fn prim_newline(_args: &[Value]) -> Result<Value, String> {
    println!();
    Ok(Value::Nil)
}
