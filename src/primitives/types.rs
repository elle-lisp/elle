//! Type checking primitives
use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
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
pub(crate) fn prim_is_pointer(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("pointer?: expected 1 argument, got {}", args.len()),
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

/// Check if value is a function (closure or primitive)
pub(crate) fn prim_is_function(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("function?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (
        SIG_OK,
        Value::bool(args[0].is_closure() || args[0].is_native_fn()),
    )
}

/// Check if value is a built-in primitive function
pub(crate) fn prim_is_primitive(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("primitive?: expected 1 argument, got {}", args.len()),
            ),
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

pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "nil?",
        func: prim_is_nil,
        effect: Effect::inert(),
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
        effect: Effect::inert(),
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
        effect: Effect::inert(),
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
        effect: Effect::inert(),
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
        effect: Effect::inert(),
        arity: Arity::Exact(1),
        doc: "Check if value is an integer (48-bit signed, range ±2^47).",
        params: &["value"],
        category: "predicate",
        example: "(integer? 42) #=> true\n(integer? 3.14) #=> false",
        aliases: &["int?"],
    },
    PrimitiveDef {
        name: "float?",
        func: prim_is_float,
        effect: Effect::inert(),
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
        effect: Effect::inert(),
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
        effect: Effect::inert(),
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
        effect: Effect::inert(),
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
        effect: Effect::inert(),
        arity: Arity::Exact(1),
        doc: "Check if value is a keyword.",
        params: &["value"],
        category: "predicate",
        example: "(keyword? :foo) #=> true\n(keyword? 42) #=> false",
        aliases: &[],
    },
    PrimitiveDef {
        name: "type",
        func: prim_type_of,
        effect: Effect::inert(),
        arity: Arity::Exact(1),
        doc: "Get the type of a value as a keyword.",
        params: &["value"],
        category: "predicate",
        example: "(type 42) #=> :integer\n(type \"hello\") #=> :string",
        aliases: &["type-of"],
    },
    PrimitiveDef {
        name: "pointer?",
        func: prim_is_pointer,
        effect: Effect::inert(),
        arity: Arity::Exact(1),
        doc: "Check if value is a raw C pointer.",
        params: &["value"],
        category: "predicate",
        example: "(pointer? ptr) #=> true\n(pointer? 42) #=> false",
        aliases: &[],
    },
    PrimitiveDef {
        name: "array?",
        func: prim_is_array,
        effect: Effect::inert(),
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
        effect: Effect::inert(),
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
        effect: Effect::inert(),
        arity: Arity::Exact(1),
        doc: "Check if value is bytes (immutable or mutable binary data).",
        params: &["value"],
        category: "predicate",
        example: "(bytes? (bytes 1 2 3)) #=> true\n(bytes? (@bytes 1 2 3)) #=> true\n(bytes? 42) #=> false",
        aliases: &[],
    },
    PrimitiveDef {
        name: "function?",
        func: prim_is_function,
        effect: Effect::inert(),
        arity: Arity::Exact(1),
        doc: "Check if value is a function (closure or primitive).",
        params: &["value"],
        category: "predicate",
        example: "(function? +) #=> true\n(function? 42) #=> false",
        aliases: &["fn?"],
    },
    PrimitiveDef {
        name: "primitive?",
        func: prim_is_primitive,
        effect: Effect::inert(),
        arity: Arity::Exact(1),
        doc: "Check if value is a built-in primitive function.",
        params: &["value"],
        category: "predicate",
        example: "(primitive? +) #=> true\n(primitive? (fn (x) x)) #=> false",
        aliases: &[],
    },
    PrimitiveDef {
        name: "mutable?",
        func: prim_is_mutable,
        effect: Effect::inert(),
        arity: Arity::Exact(1),
        doc: "Check if value is mutable (can be modified in-place).",
        params: &["value"],
        category: "predicate",
        example: "(mutable? @[1 2 3]) #=> true\n(mutable? [1 2 3]) #=> false",
        aliases: &[],
    },
    PrimitiveDef {
        name: "zero?",
        func: prim_is_zero,
        effect: Effect::inert(),
        arity: Arity::Exact(1),
        doc: "Check if value is numerically zero.",
        params: &["value"],
        category: "predicate",
        example: "(zero? 0) #=> true\n(zero? 0.0) #=> true\n(zero? 1) #=> false",
        aliases: &[],
    },
];
