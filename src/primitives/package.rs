use crate::value::{list, Condition, Value};

/// Get the current package version
pub fn prim_package_version(_args: &[Value]) -> Result<Value, Condition> {
    Ok(Value::string("0.3.0"))
}

/// Get package information
pub fn prim_package_info(_args: &[Value]) -> Result<Value, Condition> {
    Ok(list(vec![
        Value::string("Elle"),
        Value::string("0.3.0"),
        Value::string("A Lisp interpreter with bytecode compilation"),
    ]))
}
