//! Meta-programming primitives (gensym)
use crate::value::fiber::{SignalBits, SIG_OK};
use crate::value::Value;
use std::sync::atomic::{AtomicU32, Ordering};

static GENSYM_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Generate a unique symbol
pub fn prim_gensym(args: &[Value]) -> (SignalBits, Value) {
    let prefix = if args.is_empty() {
        "G".to_string()
    } else if let Some(s) = args[0].as_string() {
        s.to_string()
    } else if let Some(id) = args[0].as_symbol() {
        format!("G{}", id)
    } else {
        "G".to_string()
    };

    let counter = GENSYM_COUNTER.fetch_add(1, Ordering::SeqCst);
    let sym_name = format!("{}{}", prefix, counter);
    (SIG_OK, Value::string(sym_name))
}
