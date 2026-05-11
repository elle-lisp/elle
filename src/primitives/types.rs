//! Type checking primitives
use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;

/// Generate a type predicate that returns `(SIG_OK, Value::bool(...))`.
macro_rules! predicate {
    ($name:ident, $test:expr) => {
        pub(crate) fn $name(args: &[Value]) -> (SignalBits, Value) {
            (SIG_OK, Value::bool($test(&args[0])))
        }
    };
}

// ── Predicates that stay in Rust ────────────────────────────────────

predicate!(prim_ptr_predicate, |v: &Value| v.is_pointer()
    || v.as_managed_pointer().is_some());

predicate!(prim_is_callable, |v: &Value| v.is_closure()
    || v.is_native_fn()
    || v.as_parameter().is_some()
    || v.as_struct().is_some()
    || v.as_struct_mut().is_some()
    || v.as_array().is_some()
    || v.as_array_mut().is_some()
    || v.as_set().is_some()
    || v.as_set_mut().is_some()
    || v.is_string()
    || v.is_string_mut()
    || v.is_bytes()
    || v.is_bytes_mut());

/// Get the type name of a value as a keyword
pub(crate) fn prim_type_of(args: &[Value]) -> (SignalBits, Value) {
    let type_name = args[0].type_name();
    (SIG_OK, Value::keyword(type_name))
}

// ── Primitive table ─────────────────────────────────────────────────

pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "type-of",
        func: prim_type_of,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get the type of a value as a keyword.",
        params: &["value"],
        category: "predicate",
        example: "(type-of 42) #=> :integer\n(type-of \"hello\") #=> :string",
        aliases: &["type"],
    },
    PrimitiveDef {
        name: "ptr?",
        func: prim_ptr_predicate,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Check if value is a raw C pointer.",
        params: &["value"],
        category: "predicate",
        example: "(ptr? ptr) #=> true\n(ptr? 42) #=> false",
        aliases: &["pointer?"],
    },
    PrimitiveDef {
        name: "callable?",
        func: prim_is_callable,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "Returns true if value can be called: closures, native functions, parameters, structs, arrays, sets, strings, and bytes.",
        params: &["x"],
        category: "types",
        example: "(callable? +) #=> true\n(callable? {:a 1}) #=> true\n(callable? |1 2|) #=> true\n(callable? [1 2]) #=> true\n(callable? 42) #=> false",
        aliases: &[],
    },
];
