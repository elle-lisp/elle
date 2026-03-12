//! Function introspection primitives

use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK, SIG_QUERY};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// (closure? value) — true if value is a bytecode closure
pub(crate) fn prim_is_closure(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("closure?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (SIG_OK, Value::bool(args[0].as_closure().is_some()))
}

/// (jit? value) — true if closure has JIT-compiled code
pub(crate) fn prim_is_jit(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("jit?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(closure) = args[0].as_closure() {
        (SIG_OK, Value::bool(closure.template.jit_code.is_some()))
    } else {
        (SIG_OK, Value::FALSE)
    }
}

/// (silent? value) — true if closure is silent (does not suspend: no yield/debug/polymorphic)
pub(crate) fn prim_is_silent(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("silent?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(closure) = args[0].as_closure() {
        (SIG_OK, Value::bool(!closure.signal().may_suspend()))
    } else {
        (SIG_OK, Value::FALSE)
    }
}

/// (mutates-params? value) — true if closure mutates any parameters
pub(crate) fn prim_mutates_params(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("mutates-params?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(closure) = args[0].as_closure() {
        (SIG_OK, Value::bool(closure.template.lbox_params_mask != 0))
    } else {
        (SIG_OK, Value::FALSE)
    }
}

/// (fn/errors? value) — true if closure may error
pub(crate) fn prim_errors(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("fn/errors?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(closure) = args[0].as_closure() {
        (SIG_OK, Value::bool(closure.template.signal.may_error()))
    } else {
        (SIG_OK, Value::FALSE)
    }
}

/// (arity value) — closure arity as int, pair, or nil
pub(crate) fn prim_arity(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("arity: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(closure) = args[0].as_closure() {
        let result = match closure.template.arity {
            Arity::Exact(n) => Value::int(n as i64),
            Arity::AtLeast(n) => Value::cons(Value::int(n as i64), Value::NIL),
            Arity::Range(min, max) => Value::cons(Value::int(min as i64), Value::int(max as i64)),
        };
        (SIG_OK, result)
    } else {
        (SIG_OK, Value::NIL)
    }
}

/// (captures value) — number of captured variables, or nil
pub(crate) fn prim_captures(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("captures: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(closure) = args[0].as_closure() {
        (SIG_OK, Value::int(closure.env.len() as i64))
    } else {
        (SIG_OK, Value::NIL)
    }
}

/// (bytecode-size value) — size of bytecode in bytes, or nil
pub(crate) fn prim_bytecode_size(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("bytecode-size: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(closure) = args[0].as_closure() {
        (SIG_OK, Value::int(closure.template.bytecode.len() as i64))
    } else {
        (SIG_OK, Value::NIL)
    }
}

/// (doc target) — look up documentation for a closure, primitive, special form, or macro
///
/// If `target` is a closure, returns its docstring directly (or "No documentation found").
/// If `target` is a string or keyword, sends a SIG_QUERY to the VM to look up
/// builtin docs by name.
pub(crate) fn prim_doc(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("doc: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    // Closure: extract docstring directly — no VM query needed.
    if let Some(closure) = args[0].as_closure() {
        return if let Some(doc) = closure.template.doc {
            (SIG_OK, doc)
        } else {
            let name = closure.template.name.as_deref().unwrap_or("<anonymous>");
            (
                SIG_OK,
                Value::string(format!("No documentation found for '{}'", name)),
            )
        };
    }
    // String or keyword: look up builtin docs via SIG_QUERY.
    (SIG_QUERY, Value::cons(Value::keyword("doc"), args[0]))
}

/// (vm/query op arg) — query VM state
///
/// The single gateway to SIG_QUERY. `op` is a string or keyword
/// naming the operation; `arg` is the operation-specific argument.
/// The VM's dispatch_query handles the rest.
///
/// Operations:
/// - "call-count" closure → int
/// - "doc" name → string
/// - "global?" symbol → bool
/// - "fiber/self" _ → fiber or nil
pub(crate) fn prim_vm_query(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("vm/query: expected 2 arguments, got {}", args.len()),
            ),
        );
    }
    if !args[0].is_string() && args[0].as_keyword_name().is_none() {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "vm/query: operation must be a string or keyword, got {}",
                    args[0].type_name()
                ),
            ),
        );
    }
    (SIG_QUERY, Value::cons(args[0], args[1]))
}

/// (signals) — return the signal registry as a struct mapping keywords to bit positions
pub(crate) fn prim_signals(args: &[Value]) -> (SignalBits, Value) {
    if !args.is_empty() {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("signals: expected 0 arguments, got {}", args.len()),
            ),
        );
    }
    let reg = crate::signals::registry::global_registry().lock().unwrap();
    let mut map = std::collections::BTreeMap::new();
    for entry in reg.entries() {
        let key = crate::value::TableKey::from_value(&Value::keyword(&entry.name)).unwrap();
        map.insert(key, Value::int(entry.bit_position as i64));
    }
    (SIG_OK, Value::struct_from(map))
}

/// (keyword str) — convert a string to a keyword
///
/// Creates a content-addressed keyword from the string name.
pub(crate) fn prim_keyword(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("keyword: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(kw) = args[0].with_string(Value::keyword) {
        (SIG_OK, kw)
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("keyword: expected string, got {}", args[0].type_name()),
            ),
        )
    }
}

/// Declarative primitive definitions for introspection operations.
pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "closure?",
        func: prim_is_closure,
        signal: Signal::inert(),
        arity: Arity::Exact(1),
        doc: "Returns true if value is a bytecode closure",
        params: &["value"],
        category: "predicate",
        example: "(closure? (fn (x) x))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "jit?",
        func: prim_is_jit,
        signal: Signal::inert(),
        arity: Arity::Exact(1),
        doc: "Returns true if closure has JIT-compiled code",
        params: &["value"],
        category: "predicate",
        example: "(jit? (fn (x) x))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "silent?",
        func: prim_is_silent,
        signal: Signal::inert(),
        arity: Arity::Exact(1),
        doc: "Returns true if closure is silent (does not suspend: no yield, debug, or polymorphic signal). False for non-closures.",
        params: &["value"],
        category: "predicate",
        example: "(silent? (fn (x) x))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "coroutine?",
        func: crate::primitives::coroutines::prim_is_coroutine,
        signal: Signal::inert(),
        arity: Arity::Exact(1),
        doc: "Returns true if value is a coroutine (fiber-based)",
        params: &["value"],
        category: "predicate",
        example: "(coroutine? (coro/new (fn () 42)))",
        aliases: &["coro?"],
    },
    PrimitiveDef {
        name: "fn/mutates-params?",
        func: prim_mutates_params,
        signal: Signal::inert(),
        arity: Arity::Exact(1),
        doc: "Returns true if closure mutates any parameters",
        params: &["value"],
        category: "fn",
        example: "(fn/mutates-params? (fn (x) (assign x 1)))",
        aliases: &["mutates-params?"],
    },
    PrimitiveDef {
        name: "fn/errors?",
        func: prim_errors,
        signal: Signal::inert(),
        arity: Arity::Exact(1),
        doc: "Returns true if closure may error",
        params: &["value"],
        category: "fn",
        example: "(fn/errors? (fn (x) (/ 1 x)))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "fn/arity",
        func: prim_arity,
        signal: Signal::inert(),
        arity: Arity::Exact(1),
        doc: "Returns closure arity as int, pair, or nil",
        params: &["value"],
        category: "fn",
        example: "(fn/arity (fn (x y) x))",
        aliases: &["arity"],
    },
    PrimitiveDef {
        name: "fn/captures",
        func: prim_captures,
        signal: Signal::inert(),
        arity: Arity::Exact(1),
        doc: "Returns number of captured variables, or nil",
        params: &["value"],
        category: "fn",
        example: "(fn/captures (let ((x 1)) (fn () x)))",
        aliases: &["captures"],
    },
    PrimitiveDef {
        name: "fn/bytecode-size",
        func: prim_bytecode_size,
        signal: Signal::inert(),
        arity: Arity::Exact(1),
        doc: "Returns size of bytecode in bytes, or nil",
        params: &["value"],
        category: "fn",
        example: "(fn/bytecode-size (fn (x) x))",
        aliases: &["bytecode-size"],
    },
    PrimitiveDef {
        name: "doc",
        func: prim_doc,
        signal: Signal::inert(),
        arity: Arity::Exact(1),
        doc: "Look up documentation for a closure (by value) or a builtin (by name string).",
        params: &["target"],
        category: "meta",
        example: "(doc my-fn)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "vm/query",
        func: prim_vm_query,
        signal: Signal::inert(),
        arity: Arity::Exact(2),
        doc: "Query VM state (call-count, doc, global?, fiber/self)",
        params: &["op", "arg"],
        category: "meta",
        example: "(vm/query \"call-count\" some-fn)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "signals",
        func: prim_signals,
        signal: Signal::inert(),
        arity: Arity::Exact(0),
        doc: "Return the signal registry as a struct mapping keywords to bit positions.",
        params: &[],
        category: "meta",
        example: "(signals)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "keyword",
        func: prim_keyword,
        signal: Signal::inert(),
        arity: Arity::Exact(1),
        doc: "Convert a string to a keyword.",
        params: &["str"],
        category: "conversion",
        example: "(keyword \"foo\")",
        aliases: &["string->keyword"],
    },
];
