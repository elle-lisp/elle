use crate::error::LResult;
use crate::value::Value;

/// Display values to standard output
pub fn prim_display(args: &[Value]) -> LResult<Value> {
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
        Value::Keyword(id) => {
            // Try to get the keyword name from the thread-local symbol table
            unsafe {
                if let Some(symbols_ptr) = crate::ffi::primitives::context::get_symbol_table() {
                    if let Some(name) = (*symbols_ptr).name(*id) {
                        return format!(":{}", name);
                    }
                }
            }
            // Fallback if symbol table is not available
            format!(":keyword-{}", id.0)
        }
        Value::Cons(cons) => {
            // Format lists with proper symbol resolution
            let mut result = String::from("(");
            let mut current = Value::Cons(cons.clone());
            let mut first = true;
            while let Value::Cons(ref c) = current {
                if !first {
                    result.push(' ');
                }
                first = false;
                result.push_str(&format_value(&c.first));
                match &c.rest {
                    Value::Nil => break,
                    Value::Cons(_) => {
                        current = c.rest.clone();
                    }
                    other => {
                        result.push_str(" . ");
                        result.push_str(&format_value(other));
                        break;
                    }
                }
            }
            result.push(')');
            result
        }
        Value::Vector(vec) => {
            // Format vectors with proper symbol resolution
            let mut result = String::from("[");
            for (i, v) in vec.iter().enumerate() {
                if i > 0 {
                    result.push(' ');
                }
                result.push_str(&format_value(v));
            }
            result.push(']');
            result
        }
        _ => format!("{}", value),
    }
}

/// Print a newline
pub fn prim_newline(_args: &[Value]) -> LResult<Value> {
    println!();
    Ok(Value::Nil)
}
