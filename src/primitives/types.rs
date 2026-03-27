//! Type checking primitives
use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// Check if value is nil
pub(crate) fn prim_is_nil(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("nil?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (SIG_OK, Value::bool(args[0].is_nil()))
}

/// Check if value is a pair (cons cell)
pub(crate) fn prim_is_pair(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("pair?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let is_pair = args[0].as_cons().is_some()
        || args[0].as_syntax().is_some_and(
            |s| matches!(s.kind, crate::syntax::SyntaxKind::List(ref items) if !items.is_empty()),
        );
    (SIG_OK, Value::bool(is_pair))
}

/// Check if value is a list (empty list or cons cell)
pub(crate) fn prim_is_list(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("list?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let is_list = args[0].is_empty_list()
        || args[0].as_cons().is_some()
        || args[0]
            .as_syntax()
            .is_some_and(|s| matches!(s.kind, crate::syntax::SyntaxKind::List(_)));
    (SIG_OK, Value::bool(is_list))
}

/// Check if value is a number
pub(crate) fn prim_is_number(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("number?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (SIG_OK, Value::bool(args[0].is_number()))
}

/// Check if value is an integer
pub(crate) fn prim_is_integer(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("integer?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (SIG_OK, Value::bool(args[0].is_int()))
}

/// Check if value is a float
pub(crate) fn prim_is_float(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("float?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (SIG_OK, Value::bool(args[0].is_float()))
}

/// Check if value is a symbol
pub(crate) fn prim_is_symbol(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("symbol?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let is_symbol = args[0].is_symbol()
        || args[0]
            .as_syntax()
            .is_some_and(|s| matches!(s.kind, crate::syntax::SyntaxKind::Symbol(_)));
    (SIG_OK, Value::bool(is_symbol))
}

/// Check if value is a string (immutable or mutable)
pub(crate) fn prim_is_string(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("string?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (
        SIG_OK,
        Value::bool(args[0].is_string() || args[0].is_string_mut()),
    )
}

/// Check if value is a boolean
pub(crate) fn prim_is_boolean(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("boolean?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (SIG_OK, Value::bool(args[0].is_bool()))
}

/// Check if value is a keyword
pub(crate) fn prim_is_keyword(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("keyword?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (SIG_OK, Value::bool(args[0].is_keyword()))
}

/// Check if value is a keyword
/// Get the type name of a value as a keyword
pub(crate) fn prim_type_of(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("type-of: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    let type_name = args[0].type_name();
    (SIG_OK, Value::keyword(type_name))
}

/// Check if value is a raw C pointer
pub(crate) fn prim_ptr_predicate(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("ptr?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (
        SIG_OK,
        Value::bool(args[0].is_pointer() || args[0].as_managed_pointer().is_some()),
    )
}

/// Check if value is an array (immutable or mutable indexed sequence)
pub(crate) fn prim_is_array(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("array?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (
        SIG_OK,
        Value::bool(args[0].as_array().is_some() || args[0].as_array_mut().is_some()),
    )
}

/// Check if value is bytes (immutable or mutable binary data)
pub(crate) fn prim_is_bytes(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("bytes?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (
        SIG_OK,
        Value::bool(args[0].is_bytes() || args[0].is_bytes_mut()),
    )
}

/// Check if value is a struct (immutable or mutable key-value map)
pub(crate) fn prim_is_struct(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("struct?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (
        SIG_OK,
        Value::bool(args[0].as_struct().is_some() || args[0].as_struct_mut().is_some()),
    )
}

/// Check if value is callable (closure or native function)
pub(crate) fn prim_is_fn(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val("arity-error", "fn?: expected 1 argument"),
        );
    }
    (
        SIG_OK,
        Value::bool(args[0].is_closure() || args[0].is_native_fn()),
    )
}

/// Check if value is a native (built-in) function
pub(crate) fn prim_is_native_fn(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val("arity-error", "native-fn?: expected 1 argument"),
        );
    }
    (SIG_OK, Value::bool(args[0].is_native_fn()))
}

/// Check if value is mutable (can be modified in-place)
pub(crate) fn prim_is_mutable(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("mutable?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (SIG_OK, Value::bool(args[0].is_mutable()))
}

pub(crate) fn prim_is_immutable(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("immutable?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (SIG_OK, Value::bool(!args[0].is_mutable()))
}

/// Check if value is numerically zero
pub(crate) fn prim_is_zero(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("zero?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let is_zero = if let Some(i) = args[0].as_int() {
        i == 0
    } else if let Some(f) = args[0].as_float() {
        f == 0.0
    } else {
        false
    };
    (SIG_OK, Value::bool(is_zero))
}

pub(crate) fn prim_is_nonzero(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("nonzero?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let is_nonzero = if let Some(i) = args[0].as_int() {
        i != 0
    } else if let Some(f) = args[0].as_float() {
        f != 0.0
    } else {
        true
    };
    (SIG_OK, Value::bool(is_nonzero))
}

/// Check if value is NaN
pub(crate) fn prim_is_nan(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("nan?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let is_nan = args[0].as_number().map(|n| n.is_nan()).unwrap_or(false);
    (SIG_OK, Value::bool(is_nan))
}

/// Check if value is positive
pub(crate) fn prim_is_pos(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("pos?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(i) = args[0].as_int() {
        return (SIG_OK, Value::bool(i > 0));
    }
    if let Some(f) = args[0].as_number() {
        return (SIG_OK, Value::bool(f > 0.0));
    }
    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!("pos?: expected number, got {}", args[0].type_name()),
        ),
    )
}

/// Check if value is negative
pub(crate) fn prim_is_neg(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("neg?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(i) = args[0].as_int() {
        return (SIG_OK, Value::bool(i < 0));
    }
    if let Some(f) = args[0].as_number() {
        return (SIG_OK, Value::bool(f < 0.0));
    }
    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!("neg?: expected number, got {}", args[0].type_name()),
        ),
    )
}

/// Check if value is infinite
pub(crate) fn prim_is_inf(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("inf?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let is_inf = args[0]
        .as_number()
        .map(|n| n.is_infinite())
        .unwrap_or(false);
    (SIG_OK, Value::bool(is_inf))
}

pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "nil?",
        func: prim_is_nil,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Check if value is nil.",
        params: &["value"],
        category: "predicate",
        example: "(nil? nil) #=> true\n(nil? 42) #=> false",
        aliases: &[],
    },
    PrimitiveDef {
        name: "pair?",
        func: prim_is_pair,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Check if value is a pair (cons cell).",
        params: &["value"],
        category: "predicate",
        example: "(pair? (cons 1 2)) #=> true\n(pair? 42) #=> false",
        aliases: &[],
    },
    PrimitiveDef {
        name: "list?",
        func: prim_is_list,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Check if value is a list (empty list or cons cell).",
        params: &["value"],
        category: "predicate",
        example: "(list? (list 1 2)) #=> true\n(list? 42) #=> false",
        aliases: &[],
    },
    PrimitiveDef {
        name: "number?",
        func: prim_is_number,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Check if value is a number.",
        params: &["value"],
        category: "predicate",
        example: "(number? 42) #=> true\n(number? \"hello\") #=> false",
        aliases: &[],
    },
    PrimitiveDef {
        name: "integer?",
        func: prim_is_integer,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Check if value is an integer (full-range i64).",
        params: &["value"],
        category: "predicate",
        example: "(integer? 42) #=> true\n(integer? 3.14) #=> false",
        aliases: &["int?"],
    },
    PrimitiveDef {
        name: "float?",
        func: prim_is_float,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Check if value is a floating-point number.",
        params: &["value"],
        category: "predicate",
        example: "(float? 3.14) #=> true\n(float? 42) #=> false",
        aliases: &[],
    },
    PrimitiveDef {
        name: "symbol?",
        func: prim_is_symbol,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Check if value is a symbol.",
        params: &["value"],
        category: "predicate",
        example: "(symbol? 'foo) #=> true\n(symbol? 42) #=> false",
        aliases: &[],
    },
    PrimitiveDef {
        name: "string?",
        func: prim_is_string,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Check if value is a string (immutable or mutable).",
        params: &["value"],
        category: "predicate",
        example:
            "(string? \"hello\") #=> true\n(string? @\"hello\") #=> true\n(string? 42) #=> false",
        aliases: &[],
    },
    PrimitiveDef {
        name: "boolean?",
        func: prim_is_boolean,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Check if value is a boolean.",
        params: &["value"],
        category: "predicate",
        example: "(boolean? true) #=> true\n(boolean? 42) #=> false",
        aliases: &["bool?"],
    },
    PrimitiveDef {
        name: "keyword?",
        func: prim_is_keyword,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Check if value is a keyword.",
        params: &["value"],
        category: "predicate",
        example: "(keyword? :foo) #=> true\n(keyword? 42) #=> false",
        aliases: &[],
    },
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
        name: "array?",
        func: prim_is_array,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Check if value is an array (immutable or mutable indexed sequence).",
        params: &["value"],
        category: "predicate",
        example: "(array? [1 2 3]) #=> true\n(array? @[1 2 3]) #=> true\n(array? 42) #=> false",
        aliases: &["tuple?"],
    },
    PrimitiveDef {
        name: "struct?",
        func: prim_is_struct,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Check if value is a struct (immutable or mutable key-value map).",
        params: &["value"],
        category: "predicate",
        example: "(struct? {:a 1}) #=> true\n(struct? @{:a 1}) #=> true\n(struct? 42) #=> false",
        aliases: &["table?"],
    },
    PrimitiveDef {
        name: "bytes?",
        func: prim_is_bytes,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Check if value is bytes (immutable or mutable binary data).",
        params: &["value"],
        category: "predicate",
        example: "(bytes? (bytes 1 2 3)) #=> true\n(bytes? (@bytes 1 2 3)) #=> true\n(bytes? 42) #=> false",
        aliases: &[],
    },
    PrimitiveDef {
        name: "fn?",
        func: prim_is_fn,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "Returns true if value is callable (closure or native function).",
        params: &["x"],
        category: "types",
        example: "(fn? +) #=> true\n(fn? (fn [x] x)) #=> true\n(fn? 42) #=> false",
        aliases: &[],
    },
    PrimitiveDef {
        name: "native-fn?",
        func: prim_is_native_fn,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "Returns true if value is a native (built-in) function.",
        params: &["x"],
        category: "types",
        example: "(native-fn? +) #=> true\n(native-fn? (fn [x] x)) #=> false",
        aliases: &["native?", "primitive?"],
    },
    PrimitiveDef {
        name: "mutable?",
        func: prim_is_mutable,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Check if value is mutable (can be modified in-place).",
        params: &["value"],
        category: "predicate",
        example: "(mutable? @[1 2 3]) #=> true\n(mutable? [1 2 3]) #=> false",
        aliases: &[],
    },
    PrimitiveDef {
        name: "immutable?",
        func: prim_is_immutable,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Check if value is immutable (cannot be modified in-place).",
        params: &["value"],
        category: "predicate",
        example: "(immutable? [1 2 3]) #=> true\n(immutable? @[1 2 3]) #=> false",
        aliases: &[],
    },
    PrimitiveDef {
        name: "zero?",
        func: prim_is_zero,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Check if value is numerically zero.",
        params: &["value"],
        category: "predicate",
        example: "(zero? 0) #=> true\n(zero? 0.0) #=> true\n(zero? 1) #=> false",
        aliases: &[],
    },
    PrimitiveDef {
        name: "nonzero?",
        func: prim_is_nonzero,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Check if value is numerically nonzero.",
        params: &["value"],
        category: "predicate",
        example: "(nonzero? 1) #=> true\n(nonzero? 0) #=> false\n(nonzero? 0.0) #=> false",
        aliases: &[],
    },
    PrimitiveDef {
        name: "nan?",
        func: prim_is_nan,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Check if value is NaN (not a number). Returns false for non-numbers.",
        params: &["value"],
        category: "predicate",
        example: "(nan? (/ 0.0 0.0)) #=> true\n(nan? 1.0) #=> false\n(nan? 42) #=> false",
        aliases: &[],
    },
    PrimitiveDef {
        name: "pos?",
        func: prim_is_pos,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Check if number is positive (greater than zero).",
        params: &["value"],
        category: "predicate",
        example: "(pos? 1) #=> true\n(pos? 0) #=> false\n(pos? -1) #=> false",
        aliases: &["positive?"],
    },
    PrimitiveDef {
        name: "neg?",
        func: prim_is_neg,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Check if number is negative (less than zero).",
        params: &["value"],
        category: "predicate",
        example: "(neg? -1) #=> true\n(neg? 0) #=> false\n(neg? 1) #=> false",
        aliases: &["negative?"],
    },
    PrimitiveDef {
        name: "inf?",
        func: prim_is_inf,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Check if value is infinite. Returns false for non-numbers.",
        params: &["value"],
        category: "predicate",
        example: "(inf? (/ 1.0 0.0)) #=> true\n(inf? 1.0) #=> false\n(inf? 42) #=> false",
        aliases: &["infinite?"],
    },
];
