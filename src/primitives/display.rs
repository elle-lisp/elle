use crate::value::Value;

/// Display values to standard output
pub fn prim_display(args: &[Value]) -> Result<Value, String> {
    for arg in args {
        print!("{}", arg);
    }
    Ok(Value::Nil)
}

/// Print a newline
pub fn prim_newline(_args: &[Value]) -> Result<Value, String> {
    println!();
    Ok(Value::Nil)
}
