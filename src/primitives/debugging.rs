//! Debugging toolkit primitives
//!
//! Provides introspection and profiling capabilities:
//! - Closure introspection (arity, captures, bytecode size, effects)
//! - Time measurement (instant, duration, CPU time)
//! - Bytecode and JIT disassembly

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

/// (vm/query op arg) — query VM state
///
/// The single gateway to SIG_QUERY. `op` is a string or keyword
/// naming the operation; `arg` is the operation-specific argument.
/// The VM's dispatch_query handles the rest.
///
/// Operations:
/// - "call-count" closure → int
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
