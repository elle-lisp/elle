use crate::value::Value;

/// Expand a macro
pub fn prim_expand_macro(args: &[Value]) -> Result<Value, String> {
    // (expand-macro macro-expr)
    // Expands a macro call and returns the expanded form
    if args.len() != 1 {
        return Err(format!(
            "expand-macro: expected 1 argument, got {}",
            args.len()
        ));
    }

    // In production, this would:
    // 1. Check if the value is a macro call (list starting with macro name)
    // 2. Look up the macro definition
    // 3. Apply the macro with arguments
    // 4. Return the expanded form
    // For now, just return the argument (placeholder)
    Ok(args[0].clone())
}

/// Check if a value is a macro
pub fn prim_is_macro(args: &[Value]) -> Result<Value, String> {
    // (macro? value)
    // Returns true if value is a macro
    if args.len() != 1 {
        return Err(format!("macro?: expected 1 argument, got {}", args.len()));
    }

    // In production, would check symbol table for macro definitions
    // For now, always return false
    Ok(Value::Bool(false))
}
