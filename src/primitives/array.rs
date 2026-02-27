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

/// Create a tuple from arguments
pub fn prim_tuple(args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::tuple(args.to_vec()))
}

/// Create an array of n elements, all set to fill
pub fn prim_array_new(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("array/new: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let n = match args[0].as_int() {
        Some(i) => {
            if i < 0 {
                return (
                    SIG_ERROR,
                    error_val("error", "array/new: size must be non-negative".to_string()),
                );
            }
            i as usize
        }
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("array/new: expected integer, got {}", args[0].type_name()),
                ),
            )
        }
    };

    let fill = args[1];
    let vec = vec![fill; n];
    (SIG_OK, Value::array(vec))
}

/// Push a value onto the end of an array or buffer (mutates in place, returns the collection)
pub fn prim_push(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("push: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    if let Some(vec_ref) = args[0].as_array() {
        let mut vec = vec_ref.borrow_mut();
        vec.push(args[1]);
        drop(vec);
        return (SIG_OK, args[0]);
    }

    if let Some(buf_ref) = args[0].as_buffer() {
        let byte = match args[1].as_int() {
            Some(n) if (0..=255).contains(&n) => n as u8,
            Some(n) => {
                return (
                    SIG_ERROR,
                    error_val(
                        "error",
                        format!("push: byte value out of range 0-255: {}", n),
                    ),
                )
            }
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "push: buffer value must be integer, got {}",
                            args[1].type_name()
                        ),
                    ),
                )
            }
        };
        buf_ref.borrow_mut().push(byte);
        return (SIG_OK, args[0]);
    }

    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "push: expected array or buffer, got {}",
                args[0].type_name()
            ),
        ),
    )
}

/// Pop a value from the end of an array or buffer (mutates in place, returns the removed element)
pub fn prim_pop(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("pop: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    if let Some(vec_ref) = args[0].as_array() {
        let mut vec = vec_ref.borrow_mut();
        match vec.pop() {
            Some(v) => {
                drop(vec);
                return (SIG_OK, v);
            }
            None => {
                drop(vec);
                return (
                    SIG_ERROR,
                    error_val("error", "pop: empty array".to_string()),
                );
            }
        }
    }

    if let Some(buf_ref) = args[0].as_buffer() {
        let mut buf = buf_ref.borrow_mut();
        match buf.pop() {
            Some(byte) => {
                drop(buf);
                return (SIG_OK, Value::int(byte as i64));
            }
            None => {
                drop(buf);
                return (
                    SIG_ERROR,
                    error_val("error", "pop: empty buffer".to_string()),
                );
            }
        }
    }

    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!("pop: expected array or buffer, got {}", args[0].type_name()),
        ),
    )
}

/// Pop n values from the end of an array or buffer and return them as a new collection
pub fn prim_popn(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("popn: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let n = match args[1].as_int() {
        Some(i) => {
            if i < 0 {
                return (
                    SIG_ERROR,
                    error_val("error", "popn: count must be non-negative".to_string()),
                );
            }
            i as usize
        }
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("popn: expected integer, got {}", args[1].type_name()),
                ),
            )
        }
    };

    if let Some(vec_ref) = args[0].as_array() {
        let mut vec = vec_ref.borrow_mut();
        let len = vec.len();
        let remove_count = std::cmp::min(n, len);
        let removed: Vec<Value> = vec.drain(len - remove_count..).collect();
        drop(vec);
        return (SIG_OK, Value::array(removed));
    }

    if let Some(buf_ref) = args[0].as_buffer() {
        let mut buf = buf_ref.borrow_mut();
        let len = buf.len();
        let remove_count = std::cmp::min(n, len);
        let removed: Vec<u8> = buf.drain(len - remove_count..).collect();
        drop(buf);
        return (SIG_OK, Value::buffer(removed));
    }

    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "popn: expected array or buffer, got {}",
                args[0].type_name()
            ),
        ),
    )
}

/// Insert a value at an index in an array or buffer (mutates in place, returns the collection)
pub fn prim_insert(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 3 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("insert: expected 3 arguments, got {}", args.len()),
            ),
        );
    }

    let index = match args[1].as_int() {
        Some(i) => {
            if i < 0 {
                return (
                    SIG_ERROR,
                    error_val("error", "insert: index must be non-negative".to_string()),
                );
            }
            i as usize
        }
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("insert: expected integer, got {}", args[1].type_name()),
                ),
            )
        }
    };

    if let Some(vec_ref) = args[0].as_array() {
        let mut vec = vec_ref.borrow_mut();
        if index > vec.len() {
            vec.push(args[2]);
        } else {
            vec.insert(index, args[2]);
        }
        drop(vec);
        return (SIG_OK, args[0]);
    }

    if let Some(buf_ref) = args[0].as_buffer() {
        let byte = match args[2].as_int() {
            Some(n) if (0..=255).contains(&n) => n as u8,
            Some(n) => {
                return (
                    SIG_ERROR,
                    error_val(
                        "error",
                        format!("insert: byte value out of range 0-255: {}", n),
                    ),
                )
            }
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "insert: buffer value must be integer, got {}",
                            args[2].type_name()
                        ),
                    ),
                )
            }
        };
        let mut buf = buf_ref.borrow_mut();
        if index > buf.len() {
            buf.push(byte);
        } else {
            buf.insert(index, byte);
        }
        drop(buf);
        return (SIG_OK, args[0]);
    }

    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "insert: expected array or buffer, got {}",
                args[0].type_name()
            ),
        ),
    )
}

/// Remove element(s) at an index from an array or buffer (mutates in place, returns the collection)
pub fn prim_remove(args: &[Value]) -> (SignalBits, Value) {
    if args.len() < 2 || args.len() > 3 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("remove: expected 2-3 arguments, got {}", args.len()),
            ),
        );
    }

    let index = match args[1].as_int() {
        Some(i) => {
            if i < 0 {
                return (
                    SIG_ERROR,
                    error_val("error", "remove: index must be non-negative".to_string()),
                );
            }
            i as usize
        }
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("remove: expected integer, got {}", args[1].type_name()),
                ),
            )
        }
    };

    let count = if args.len() == 3 {
        match args[2].as_int() {
            Some(i) => {
                if i < 0 {
                    return (
                        SIG_ERROR,
                        error_val("error", "remove: count must be non-negative".to_string()),
                    );
                }
                i as usize
            }
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("remove: expected integer, got {}", args[2].type_name()),
                    ),
                )
            }
        }
    } else {
        1
    };

    if let Some(vec_ref) = args[0].as_array() {
        let mut vec = vec_ref.borrow_mut();
        let len = vec.len();
        if index < len {
            let remove_count = std::cmp::min(count, len - index);
            for _ in 0..remove_count {
                vec.remove(index);
            }
        }
        drop(vec);
        return (SIG_OK, args[0]);
    }

    if let Some(buf_ref) = args[0].as_buffer() {
        let mut buf = buf_ref.borrow_mut();
        let len = buf.len();
        if index < len {
            let remove_count = std::cmp::min(count, len - index);
            for _ in 0..remove_count {
                buf.remove(index);
            }
        }
        drop(buf);
        return (SIG_OK, args[0]);
    }

    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "remove: expected array or buffer, got {}",
                args[0].type_name()
            ),
        ),
    )
}

pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "tuple",
        func: prim_tuple,
        effect: Effect::none(),
        arity: Arity::AtLeast(0),
        doc: "Create an immutable tuple from arguments.",
        params: &[],
        category: "array",
        example: "(tuple 1 2 3) #=> [1 2 3]",
        aliases: &[],
    },
    PrimitiveDef {
        name: "array",
        func: prim_array,
        effect: Effect::none(),
        arity: Arity::AtLeast(0),
        doc: "Create a mutable array from arguments.",
        params: &[],
        category: "array",
        example: "(array 1 2 3) #=> @[1 2 3]",
        aliases: &[],
    },
    PrimitiveDef {
        name: "array/new",
        func: prim_array_new,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "Create array of n elements, all set to fill value.",
        params: &["n", "fill"],
        category: "array",
        example: "(array/new 3 0) #=> [0 0 0]",
        aliases: &[],
    },
    PrimitiveDef {
        name: "push",
        func: prim_push,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "Append element to end of array. Mutates in place, returns the same array.",
        params: &["arr", "val"],
        category: "array",
        example: "(push @[1 2] 3) #=> @[1 2 3]",
        aliases: &[],
    },
    PrimitiveDef {
        name: "pop",
        func: prim_pop,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Remove and return last element from array. Mutates in place.",
        params: &["arr"],
        category: "array",
        example: "(pop @[1 2 3]) #=> 3",
        aliases: &[],
    },
    PrimitiveDef {
        name: "popn",
        func: prim_popn,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "Remove and return last n elements from array as a new array. Mutates original.",
        params: &["arr", "n"],
        category: "array",
        example: "(popn @[1 2 3 4] 2) #=> @[3 4]",
        aliases: &[],
    },
    PrimitiveDef {
        name: "insert",
        func: prim_insert,
        effect: Effect::none(),
        arity: Arity::Exact(3),
        doc: "Insert element at index in array. Mutates in place, returns the same array.",
        params: &["arr", "idx", "val"],
        category: "array",
        example: "(insert @[1 3] 1 2) #=> @[1 2 3]",
        aliases: &[],
    },
    PrimitiveDef {
        name: "remove",
        func: prim_remove,
        effect: Effect::none(),
        arity: Arity::Range(2, 3),
        doc: "Remove element(s) at index from array. Mutates in place, returns the same array.",
        params: &["arr", "idx", "count"],
        category: "array",
        example: "(remove @[1 2 3] 1) #=> @[1 3]",
        aliases: &[],
    },
];
