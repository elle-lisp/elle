//! Type checking primitives
use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// Check if value is nil
pub fn prim_is_nil(args: &[Value]) -> (SignalBits, Value) {
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
pub fn prim_is_pair(args: &[Value]) -> (SignalBits, Value) {
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
pub fn prim_is_list(args: &[Value]) -> (SignalBits, Value) {
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
pub fn prim_is_number(args: &[Value]) -> (SignalBits, Value) {
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

/// Check if value is a symbol
pub fn prim_is_symbol(args: &[Value]) -> (SignalBits, Value) {
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

/// Check if value is a string
pub fn prim_is_string(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("string?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (SIG_OK, Value::bool(args[0].is_string()))
}

/// Check if value is a boolean
pub fn prim_is_boolean(args: &[Value]) -> (SignalBits, Value) {
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
pub fn prim_is_keyword(args: &[Value]) -> (SignalBits, Value) {
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
pub fn prim_type_of(args: &[Value]) -> (SignalBits, Value) {
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
pub fn prim_is_pointer(args: &[Value]) -> (SignalBits, Value) {
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

/// Check if value is an array (mutable indexed sequence)
pub fn prim_is_array(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("array?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (SIG_OK, Value::bool(args[0].as_array().is_some()))
}

/// Check if value is a tuple (immutable indexed sequence)
pub fn prim_is_tuple(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("tuple?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (SIG_OK, Value::bool(args[0].as_tuple().is_some()))
}

/// Check if value is a table (mutable key-value map)
pub fn prim_is_table(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("table?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (SIG_OK, Value::bool(args[0].as_table().is_some()))
}

/// Check if value is a buffer (mutable byte sequence)
pub fn prim_is_buffer(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("buffer?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (SIG_OK, Value::bool(args[0].is_buffer()))
}

/// Check if value is bytes (immutable binary data)
pub fn prim_is_bytes(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("bytes?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (SIG_OK, Value::bool(args[0].is_bytes()))
}

/// Check if value is a blob (mutable binary data)
pub fn prim_is_blob(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("blob?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (SIG_OK, Value::bool(args[0].is_blob()))
}

/// Check if value is a struct (immutable key-value map)
pub fn prim_is_struct(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("struct?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (SIG_OK, Value::bool(args[0].as_struct().is_some()))
}

pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "nil?",
        func: prim_is_nil,
        effect: Effect::none(),
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
        effect: Effect::none(),
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
        effect: Effect::none(),
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
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Check if value is a number.",
        params: &["value"],
        category: "predicate",
        example: "(number? 42) #=> true\n(number? \"hello\") #=> false",
        aliases: &[],
    },
    PrimitiveDef {
        name: "symbol?",
        func: prim_is_symbol,
        effect: Effect::none(),
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
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Check if value is a string.",
        params: &["value"],
        category: "predicate",
        example: "(string? \"hello\") #=> true\n(string? 42) #=> false",
        aliases: &[],
    },
    PrimitiveDef {
        name: "boolean?",
        func: prim_is_boolean,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Check if value is a boolean.",
        params: &["value"],
        category: "predicate",
        example: "(boolean? true) #=> true\n(boolean? 42) #=> false",
        aliases: &[],
    },
    PrimitiveDef {
        name: "keyword?",
        func: prim_is_keyword,
        effect: Effect::none(),
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
        effect: Effect::none(),
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
        effect: Effect::none(),
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
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Check if value is an array (mutable indexed sequence).",
        params: &["value"],
        category: "predicate",
        example: "(array? @[1 2 3]) #=> true\n(array? [1 2 3]) #=> false",
        aliases: &[],
    },
    PrimitiveDef {
        name: "tuple?",
        func: prim_is_tuple,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Check if value is a tuple (immutable indexed sequence).",
        params: &["value"],
        category: "predicate",
        example: "(tuple? [1 2 3]) #=> true\n(tuple? @[1 2 3]) #=> false",
        aliases: &[],
    },
    PrimitiveDef {
        name: "table?",
        func: prim_is_table,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Check if value is a table (mutable key-value map).",
        params: &["value"],
        category: "predicate",
        example: "(table? @{:a 1}) #=> true\n(table? {:a 1}) #=> false",
        aliases: &[],
    },
    PrimitiveDef {
        name: "struct?",
        func: prim_is_struct,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Check if value is a struct (immutable key-value map).",
        params: &["value"],
        category: "predicate",
        example: "(struct? {:a 1}) #=> true\n(struct? @{:a 1}) #=> false",
        aliases: &[],
    },
    PrimitiveDef {
        name: "buffer?",
        func: prim_is_buffer,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Check if value is a buffer (mutable byte sequence).",
        params: &["value"],
        category: "predicate",
        example: "(buffer? @\"hello\") #=> true\n(buffer? \"hello\") #=> false",
        aliases: &[],
    },
    PrimitiveDef {
        name: "bytes?",
        func: prim_is_bytes,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Check if value is bytes (immutable binary data).",
        params: &["value"],
        category: "predicate",
        example: "(bytes? (bytes 1 2 3)) ;=> true",
        aliases: &[],
    },
    PrimitiveDef {
        name: "blob?",
        func: prim_is_blob,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Check if value is a blob (mutable binary data).",
        params: &["value"],
        category: "predicate",
        example: "(blob? (blob 1 2 3)) ;=> true",
        aliases: &[],
    },
];
