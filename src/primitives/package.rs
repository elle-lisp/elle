use crate::value::fiber::{SignalBits, SIG_OK};
use crate::value::{list, Value};

/// Get the current package version
pub fn prim_package_version(_args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::string("0.3.0"))
}

/// Get package information
pub fn prim_package_info(_args: &[Value]) -> (SignalBits, Value) {
    (
        SIG_OK,
        list(vec![
            Value::string("Elle"),
            Value::string("0.3.0"),
            Value::string("A Lisp interpreter with bytecode compilation"),
        ]),
    )
}
