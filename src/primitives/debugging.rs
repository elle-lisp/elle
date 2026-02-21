//! Debugging toolkit primitives
//!
//! Provides introspection and profiling capabilities:
//! - Closure introspection (arity, captures, bytecode size, effects)
//! - Time measurement (instant, duration, CPU time)
//! - Bytecode and JIT disassembly

use crate::value::types::Arity;
use crate::value::{Condition, Value};

// ============================================================================
// Introspection predicates
// ============================================================================

/// (closure? value) — true if value is a bytecode closure
pub fn prim_is_closure(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "closure?: expected 1 argument, got {}",
            args.len()
        )));
    }
    Ok(Value::bool(args[0].as_closure().is_some()))
}

/// (jit? value) — true if closure has JIT-compiled code
pub fn prim_is_jit(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "jit?: expected 1 argument, got {}",
            args.len()
        )));
    }
    if let Some(closure) = args[0].as_closure() {
        Ok(Value::bool(closure.jit_code.is_some()))
    } else {
        Ok(Value::FALSE)
    }
}

/// (pure? value) — true if closure has Pure yield behavior
pub fn prim_is_pure(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "pure?: expected 1 argument, got {}",
            args.len()
        )));
    }
    if let Some(closure) = args[0].as_closure() {
        Ok(Value::bool(closure.effect.is_pure()))
    } else {
        Ok(Value::FALSE)
    }
}

/// (coro? value) — true if closure has Yields effect
pub fn prim_is_coro(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "coro?: expected 1 argument, got {}",
            args.len()
        )));
    }
    if let Some(closure) = args[0].as_closure() {
        Ok(Value::bool(closure.effect.may_yield()))
    } else {
        Ok(Value::FALSE)
    }
}

/// (mutates-params? value) — true if closure mutates any parameters
pub fn prim_mutates_params(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "mutates-params?: expected 1 argument, got {}",
            args.len()
        )));
    }
    if let Some(closure) = args[0].as_closure() {
        Ok(Value::bool(closure.cell_params_mask != 0))
    } else {
        Ok(Value::FALSE)
    }
}

/// (raises? value) — true if closure may raise
pub fn prim_raises(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "raises?: expected 1 argument, got {}",
            args.len()
        )));
    }
    if let Some(closure) = args[0].as_closure() {
        Ok(Value::bool(closure.effect.may_raise))
    } else {
        Ok(Value::FALSE)
    }
}

// ============================================================================
// Additional introspection
// ============================================================================

/// (arity value) — closure arity as int, pair, or nil
pub fn prim_arity(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "arity: expected 1 argument, got {}",
            args.len()
        )));
    }
    if let Some(closure) = args[0].as_closure() {
        let result = match closure.arity {
            Arity::Exact(n) => Value::int(n as i64),
            Arity::AtLeast(n) => Value::cons(Value::int(n as i64), Value::NIL),
            Arity::Range(min, max) => Value::cons(Value::int(min as i64), Value::int(max as i64)),
        };
        Ok(result)
    } else {
        Ok(Value::NIL)
    }
}

/// (captures value) — number of captured variables, or nil
pub fn prim_captures(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "captures: expected 1 argument, got {}",
            args.len()
        )));
    }
    if let Some(closure) = args[0].as_closure() {
        Ok(Value::int(closure.env.len() as i64))
    } else {
        Ok(Value::NIL)
    }
}

/// (bytecode-size value) — size of bytecode in bytes, or nil
pub fn prim_bytecode_size(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "bytecode-size: expected 1 argument, got {}",
            args.len()
        )));
    }
    if let Some(closure) = args[0].as_closure() {
        Ok(Value::int(closure.bytecode.len() as i64))
    } else {
        Ok(Value::NIL)
    }
}

// ============================================================================
// Disassembly
// ============================================================================

/// (disbit closure) — disassemble bytecode as vector of strings
pub fn prim_disbit(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "disbit: expected 1 argument, got {}",
            args.len()
        )));
    }
    if let Some(closure) = args[0].as_closure() {
        let mut lines = crate::compiler::disassemble_lines(&closure.bytecode);
        for (i, c) in closure.constants.iter().enumerate() {
            lines.push(format!("const[{}] = {:?}", i, c));
        }
        Ok(Value::vector(
            lines.into_iter().map(Value::string).collect(),
        ))
    } else {
        Err(Condition::type_error(
            "disbit: argument must be a closure".to_string(),
        ))
    }
}

/// (disjit closure) — return Cranelift IR as vector of strings, or nil
pub fn prim_disjit(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "disjit: expected 1 argument, got {}",
            args.len()
        )));
    }
    if let Some(closure) = args[0].as_closure() {
        let lir = match &closure.lir_function {
            Some(lir) => lir.clone(),
            None => return Ok(Value::NIL),
        };
        let compiler = match crate::jit::JitCompiler::new() {
            Ok(c) => c,
            Err(_) => return Ok(Value::NIL),
        };
        match compiler.clif_text(&lir) {
            Ok(lines) => Ok(Value::vector(
                lines.into_iter().map(Value::string).collect(),
            )),
            Err(_) => Ok(Value::NIL),
        }
    } else {
        Err(Condition::type_error(
            "disjit: argument must be a closure".to_string(),
        ))
    }
}
