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
    // SIG_QUERY to the VM, which checks jit_cache by bytecode pointer.
    (SIG_QUERY, Value::cons(Value::keyword("jit?"), args[0]))
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
        (
            SIG_OK,
            Value::bool(closure.template.capture_params_mask != 0),
        )
    } else {
        (SIG_OK, Value::FALSE)
    }
}

/// (fn/gpu-eligible? value) — true if closure is eligible for GPU compilation
pub(crate) fn prim_gpu_eligible(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("fn/gpu-eligible?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(closure) = args[0].as_closure() {
        let eligible = match &closure.template.lir_function {
            Some(lir) => lir.is_gpu_eligible(),
            None => closure.template.is_gpu_candidate(),
        };
        (SIG_OK, Value::bool(eligible))
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

/// (doc target) — look up documentation for a closure, primitive, or special form.
///
/// Dispatch:
/// - closure value → returns `closure.template.doc` (the leading string literal
///   from the function body), or "No documentation found for 'name'" if absent.
/// - string or keyword → sends SIG_QUERY "doc" to the VM, which looks up
///   `vm.docs`. Only native primitives and special forms are in `vm.docs`;
///   stdlib functions are NOT (their docstrings live in the closure value).
///
/// Usage: prefer `(doc name)` over `(doc "name")`. The analyzer rewrites
/// `(doc name)` appropriately: closures are passed through as values; native
/// primitives and special forms are rewritten to `(doc "name")` string lookup.
/// Passing an explicit string `(doc "stdlib-fn")` will NOT find stdlib docs.
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

/// (lir/closure-value-const-count) — number of closure-valued `ValueConst`
/// instructions converted to `ClosureRef` by the LIR cross-thread
/// serializer during this process's lifetime.
///
/// Used by regression tests to assert the ClosureRef LIR-transfer fix
/// is actually firing on real spawn patterns. See
/// `src/lir/types.rs::convert_value_consts_for_send`.
pub(crate) fn prim_closure_value_const_count(args: &[Value]) -> (SignalBits, Value) {
    if !args.is_empty() {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "lir/closure-value-const-count: expected 0 arguments, got {}",
                    args.len()
                ),
            ),
        );
    }
    (
        SIG_OK,
        Value::int(crate::lir::closure_value_const_count() as i64),
    )
}

/// (jit/rejections) — list closures rejected from JIT compilation with reasons
///
/// Returns a list of structs, each with :name, :reason, and :calls keys.
/// Sorted by call count ascending (coldest first).
pub(crate) fn prim_jit_rejections(args: &[Value]) -> (SignalBits, Value) {
    if !args.is_empty() {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("jit/rejections: expected 0 arguments, got {}", args.len()),
            ),
        );
    }
    (
        SIG_QUERY,
        Value::cons(Value::keyword("jit/rejections"), Value::NIL),
    )
}

/// Declarative primitive definitions for introspection operations.
pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "closure?",
        func: prim_is_closure,
        signal: Signal::errors(),
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
        signal: Signal { bits: SIG_QUERY.union(SIG_ERROR), propagates: 0 },
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
        signal: Signal::errors(),
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
        signal: Signal::silent(),
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
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns true if closure mutates any parameters",
        params: &["value"],
        category: "fn",
        example: "(fn/mutates-params? (fn (x) (assign x 1)))",
        aliases: &["mutates-params?"],
    },
    PrimitiveDef {
        name: "fn/gpu-eligible?",
        func: prim_gpu_eligible,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns true if closure passes signal and structural checks for GPU compilation",
        params: &["value"],
        category: "fn",
        example: "(fn/gpu-eligible? (fn [a b] (+ a b)))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "fn/errors?",
        func: prim_errors,
        signal: Signal::errors(),
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
        signal: Signal::errors(),
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
        signal: Signal::errors(),
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
        signal: Signal::errors(),
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
        signal: Signal { bits: SIG_QUERY.union(SIG_ERROR), propagates: 0 },
        arity: Arity::Exact(1),
        doc: "Look up documentation for a value or builtin. \
              Pass a closure (user-defined or stdlib) to extract its docstring. \
              Pass a string or keyword to look up a native primitive or special form by name. \
              Note: (doc name) works for closures and native primitives; \
              (doc \"name\") only works for native primitives and special forms.",
        params: &["target"],
        category: "meta",
        example: "(doc inc)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "vm/query",
        func: prim_vm_query,
        signal: Signal { bits: SIG_QUERY.union(SIG_ERROR), propagates: 0 },
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
        signal: Signal::errors(),
        arity: Arity::Exact(0),
        doc: "Return the signal registry as a struct mapping keywords to bit positions.",
        params: &[],
        category: "meta",
        example: "(signals)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "jit/rejections",
        func: prim_jit_rejections,
        signal: Signal { bits: SIG_QUERY.union(SIG_ERROR), propagates: 0 },
        arity: Arity::Exact(0),
        doc: "List closures rejected from JIT compilation. Returns list of {:name :reason :calls} structs sorted by call count ascending.",
        params: &[],
        category: "meta",
        example: "(jit/rejections)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "lir/closure-value-const-count",
        func: prim_closure_value_const_count,
        signal: Signal::silent(),
        arity: Arity::Exact(0),
        doc: "Number of closure-valued ValueConst instructions converted to ClosureRef by the LIR cross-thread serializer. Used by regression tests to assert the ClosureRef LIR-transfer fix fires.",
        params: &[],
        category: "meta",
        example: "(lir/closure-value-const-count)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "keyword",
        func: prim_keyword,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Convert a string to a keyword.",
        params: &["str"],
        category: "conversion",
        example: "(keyword \"foo\")",
        aliases: &["string->keyword"],
    },
];
