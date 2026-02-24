//! Debugging toolkit primitives
//!
//! Provides introspection and profiling capabilities:
//! - Closure introspection (arity, captures, bytecode size, effects)
//! - Time measurement (instant, duration, CPU time)
//! - Bytecode and JIT disassembly

use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK, SIG_QUERY};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

// ============================================================================
// Introspection predicates
// ============================================================================

/// (closure? value) — true if value is a bytecode closure
pub fn prim_is_closure(args: &[Value]) -> (SignalBits, Value) {
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
pub fn prim_is_jit(args: &[Value]) -> (SignalBits, Value) {
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
        (SIG_OK, Value::bool(closure.jit_code.is_some()))
    } else {
        (SIG_OK, Value::FALSE)
    }
}

/// (pure? value) — true if closure has Pure yield behavior
pub fn prim_is_pure(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("pure?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(closure) = args[0].as_closure() {
        (SIG_OK, Value::bool(closure.effect.is_pure()))
    } else {
        (SIG_OK, Value::FALSE)
    }
}

/// (mutates-params? value) — true if closure mutates any parameters
pub fn prim_mutates_params(args: &[Value]) -> (SignalBits, Value) {
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
        (SIG_OK, Value::bool(closure.cell_params_mask != 0))
    } else {
        (SIG_OK, Value::FALSE)
    }
}

/// (raises? value) — true if closure may raise
pub fn prim_raises(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("raises?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(closure) = args[0].as_closure() {
        (SIG_OK, Value::bool(closure.effect.may_raise()))
    } else {
        (SIG_OK, Value::FALSE)
    }
}

// ============================================================================
// Additional introspection
// ============================================================================

/// (arity value) — closure arity as int, pair, or nil
pub fn prim_arity(args: &[Value]) -> (SignalBits, Value) {
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
        let result = match closure.arity {
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
pub fn prim_captures(args: &[Value]) -> (SignalBits, Value) {
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
pub fn prim_bytecode_size(args: &[Value]) -> (SignalBits, Value) {
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
        (SIG_OK, Value::int(closure.bytecode.len() as i64))
    } else {
        (SIG_OK, Value::NIL)
    }
}

// ============================================================================
// VM-access introspection (SIG_QUERY)
// ============================================================================

/// (doc name) — look up documentation for any named form (primitive, special form, or macro)
///
/// Sends a SIG_QUERY to the VM which looks up the name in its
/// `docs` map and returns a formatted doc string.
pub fn prim_doc(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("doc: expected 1 argument, got {}", args.len()),
            ),
        );
    }
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
pub fn prim_vm_query(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("vm/query: expected 2 arguments, got {}", args.len()),
            ),
        );
    }
    if args[0].as_string().is_none() && args[0].as_keyword_name().is_none() {
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

/// (string->keyword str) — convert a string to a keyword
///
/// Creates a content-addressed keyword from the string name.
pub fn prim_string_to_keyword(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("string->keyword: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(name) = args[0].as_string() {
        (SIG_OK, Value::keyword(name))
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "string->keyword: expected string, got {}",
                    args[0].type_name()
                ),
            ),
        )
    }
}

/// (vm/list-primitives) or (vm/list-primitives :category) — list registered names
///
/// Returns a sorted list of strings. With no arguments, returns all names.
/// With a category keyword or string, filters by category (e.g., :math, :"special form").
pub fn prim_list_primitives(args: &[Value]) -> (SignalBits, Value) {
    if args.len() > 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "vm/list-primitives: expected 0-1 arguments, got {}",
                    args.len()
                ),
            ),
        );
    }
    let filter = if args.is_empty() { Value::NIL } else { args[0] };
    (
        SIG_QUERY,
        Value::cons(Value::keyword("list-primitives"), filter),
    )
}

/// (vm/primitive-meta name) — get structured metadata for a primitive
///
/// Returns a struct with keys: "name", "doc", "params", "category", "example", "arity", "effect".
/// Returns nil if the primitive is not found.
pub fn prim_primitive_meta(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("vm/primitive-meta: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    (
        SIG_QUERY,
        Value::cons(Value::keyword("primitive-meta"), args[0]),
    )
}

// ============================================================================
// Disassembly
// ============================================================================

/// (disbit closure) — disassemble bytecode as array of strings
pub fn prim_disbit(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("disbit: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(closure) = args[0].as_closure() {
        let mut lines = crate::compiler::disassemble_lines(&closure.bytecode);
        for (i, c) in closure.constants.iter().enumerate() {
            lines.push(format!("const[{}] = {:?}", i, c));
        }
        (
            SIG_OK,
            Value::array(lines.into_iter().map(Value::string).collect()),
        )
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                "disbit: argument must be a closure".to_string(),
            ),
        )
    }
}

/// (disjit closure) — return Cranelift IR as array of strings, or nil
pub fn prim_disjit(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("disjit: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(closure) = args[0].as_closure() {
        let lir = match &closure.lir_function {
            Some(lir) => lir.clone(),
            None => return (SIG_OK, Value::NIL),
        };
        let compiler = match crate::jit::JitCompiler::new() {
            Ok(c) => c,
            Err(_) => return (SIG_OK, Value::NIL),
        };
        match compiler.clif_text(&lir) {
            Ok(lines) => (
                SIG_OK,
                Value::array(lines.into_iter().map(Value::string).collect()),
            ),
            Err(_) => (SIG_OK, Value::NIL),
        }
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                "disjit: argument must be a closure".to_string(),
            ),
        )
    }
}

/// Declarative primitive definitions for debugging operations.
pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "closure?",
        func: prim_is_closure,
        effect: Effect::none(),
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
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Returns true if closure has JIT-compiled code",
        params: &["value"],
        category: "predicate",
        example: "(jit? (fn (x) x))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "pure?",
        func: prim_is_pure,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Returns true if closure has Pure effect (does not suspend)",
        params: &["value"],
        category: "predicate",
        example: "(pure? (fn (x) x))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "coro?",
        func: crate::primitives::coroutines::prim_is_coroutine,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Returns true if value is a coroutine (fiber-based)",
        params: &["value"],
        category: "predicate",
        example: "(coro? (coro/new (fn () 42)))",
        aliases: &["coroutine?"],
    },
    PrimitiveDef {
        name: "fn/mutates-params?",
        func: prim_mutates_params,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Returns true if closure mutates any parameters",
        params: &["value"],
        category: "fn",
        example: "(fn/mutates-params? (fn (x) (set! x 1)))",
        aliases: &["mutates-params?"],
    },
    PrimitiveDef {
        name: "fn/raises?",
        func: prim_raises,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Returns true if closure may raise an error",
        params: &["value"],
        category: "fn",
        example: "(fn/raises? (fn (x) (/ 1 x)))",
        aliases: &["raises?"],
    },
    PrimitiveDef {
        name: "fn/arity",
        func: prim_arity,
        effect: Effect::none(),
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
        effect: Effect::none(),
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
        effect: Effect::none(),
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
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Look up documentation for a primitive.",
        params: &["name"],
        category: "meta",
        example: "(doc \"cons\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "vm/query",
        func: prim_vm_query,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "Query VM state (call-count, doc, global?, fiber/self)",
        params: &["op", "arg"],
        category: "meta",
        example: "(vm/query \"call-count\" some-fn)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "string->keyword",
        func: prim_string_to_keyword,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Convert a string to a keyword",
        params: &["str"],
        category: "conversion",
        example: "(string->keyword \"foo\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "fn/disasm",
        func: prim_disbit,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Disassemble a closure's bytecode into a list of instruction strings.",
        params: &["closure"],
        category: "fn",
        example: "(fn/disasm (fn (x) x))",
        aliases: &["disbit", "fn/disbit"],
    },
    PrimitiveDef {
        name: "fn/disasm-jit",
        func: prim_disjit,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Disassemble a closure's JIT-compiled Cranelift IR, or nil if not JIT'd.",
        params: &["closure"],
        category: "fn",
        example: "(fn/disasm-jit (fn (x) x))",
        aliases: &["disjit", "fn/disjit"],
    },
    PrimitiveDef {
        name: "vm/list-primitives",
        func: prim_list_primitives,
        effect: Effect::none(),
        arity: Arity::Range(0, 1),
        doc: "List registered names as a sorted list of strings. Optional category filter.",
        params: &["category?"],
        category: "meta",
        example: "(vm/list-primitives)\n(vm/list-primitives :math)\n(vm/list-primitives :\"special form\")",
        aliases: &["list-primitives"],
    },
    PrimitiveDef {
        name: "vm/primitive-meta",
        func: prim_primitive_meta,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Get structured metadata for a primitive as a struct.",
        params: &["name"],
        category: "meta",
        example:
            "(struct-get (vm/primitive-meta \"cons\") \"doc\") ;=> \"Construct a cons cell...\"",
        aliases: &["primitive-meta"],
    },
];
