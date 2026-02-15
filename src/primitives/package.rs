use crate::error::LResult;
use crate::value::{list, Value};

/// Get the current package version
pub fn prim_package_version(_args: &[Value]) -> LResult<Value> {
    Ok(Value::String("0.3.0".into()))
}

/// Get package information
pub fn prim_package_info(_args: &[Value]) -> LResult<Value> {
    Ok(list(vec![
        Value::String("Elle".into()),
        Value::String("0.3.0".into()),
        Value::String("A Lisp interpreter with bytecode compilation".into()),
    ]))
}
