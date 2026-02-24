//! Array operations primitives
use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// Create an array from arguments
pub fn prim_array(args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::array(args.to_vec()))
}

/// Get a reference from an array at an index
pub fn prim_array_ref(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("array-ref: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let vec = match args[0].as_array() {
        Some(v) => v,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("array-ref: expected array, got {}", args[0].type_name()),
                ),
            )
        }
    };
    let index = match args[1].as_int() {
        Some(i) => i as usize,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("array-ref: expected integer, got {}", args[1].type_name()),
                ),
            )
        }
    };

    let borrowed = vec.borrow();
    match borrowed.get(index).cloned() {
        Some(v) => (SIG_OK, v),
        None => (
            SIG_ERROR,
            error_val(
                "error",
                format!(
                    "array-ref: index {} out of bounds (length {})",
                    index,
                    borrowed.len()
                ),
            ),
        ),
    }
}

/// Set a value in an array at an index (returns new array)
pub fn prim_array_set(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 3 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("array-set!: expected 3 arguments, got {}", args.len()),
            ),
        );
    }

    let vec_ref = match args[0].as_array() {
        Some(v) => v,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("array-set!: expected array, got {}", args[0].type_name()),
                ),
            )
        }
    };
    let index = match args[1].as_int() {
        Some(i) => i as usize,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("array-set!: expected integer, got {}", args[1].type_name()),
                ),
            )
        }
    };
    let value = args[2];

    let mut vec = vec_ref.borrow_mut();
    if index >= vec.len() {
        return (
            SIG_ERROR,
            error_val(
                "error",
                format!(
                    "array-set!: index {} out of bounds (length {})",
                    index,
                    vec.len()
                ),
            ),
        );
    }

    vec[index] = value;
    drop(vec);
    (SIG_OK, Value::array(vec_ref.borrow().clone()))
}

pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "array",
        func: prim_array,
        effect: Effect::none(),
        arity: Arity::AtLeast(0),
        doc: "Create a mutable array from arguments.",
        params: &[],
        category: "",
        example: "(array 1 2 3) ;=> [1 2 3]",
        aliases: &[],
    },
    PrimitiveDef {
        name: "array/ref",
        func: prim_array_ref,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "Get element at index from an array.",
        params: &["arr", "idx"],
        category: "array",
        example: "(array/ref (array 10 20 30) 1) ;=> 20",
        aliases: &["array-ref"],
    },
    PrimitiveDef {
        name: "array/set!",
        func: prim_array_set,
        effect: Effect::none(),
        arity: Arity::Exact(3),
        doc: "Set element at index in an array. Returns modified array.",
        params: &["arr", "idx", "val"],
        category: "array",
        example: "(array/set! (array 1 2 3) 0 99) ;=> [99 2 3]",
        aliases: &["array-set!"],
    },
];
