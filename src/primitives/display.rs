use crate::value::fiber::{SignalBits, SIG_OK};
use crate::value::Value;

/// (display val ...) — human-readable output, no quotes on strings
pub fn prim_display(args: &[Value]) -> (SignalBits, Value) {
    for arg in args {
        print!("{}", arg);
    }
    (SIG_OK, Value::NIL)
}

/// (print val ...) — machine-readable output with newline, strings quoted
pub fn prim_print(args: &[Value]) -> (SignalBits, Value) {
    for arg in args {
        print!("{:?}", arg);
    }
    println!();
    (SIG_OK, Value::NIL)
}

/// (newline) — print a newline
pub fn prim_newline(_args: &[Value]) -> (SignalBits, Value) {
    println!();
    (SIG_OK, Value::NIL)
}
