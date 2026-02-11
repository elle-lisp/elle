//! JIT compilation primitives

use crate::compiler::cranelift::jit_compile::is_jit_compilable;
use crate::value::Value;

#[cfg(test)]
use crate::value::{Arity, JitClosure};
#[cfg(test)]
use std::rc::Rc;

/// (jit-compile closure) -> jit-closure or original closure
///
/// Attempts to JIT compile a closure to native code.
/// Returns a JitClosure if successful, or the original closure if compilation
/// is not possible (e.g., unsupported constructs in the body).
///
/// Errors only on actual compilation failures, not on "not compilable" cases.
pub fn prim_jit_compile(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err(format!(
            "jit-compile: expected 1 argument, got {}",
            args.len()
        ));
    }

    let closure = match &args[0] {
        Value::Closure(c) => c,
        Value::JitClosure(_) => {
            // Already JIT compiled, return as-is
            return Ok(args[0].clone());
        }
        _ => {
            return Err(format!(
                "jit-compile: expected closure, got {}",
                args[0].type_name()
            ))
        }
    };

    // Check if source AST is available
    if closure.source_ast.is_none() {
        // No AST available, return original closure
        return Ok(args[0].clone());
    }

    // For now, we don't have a JIT context available in primitives.
    // The proper solution would be to make jit-compile a special form in the VM,
    // but for Phase 4, we'll return the original closure.
    // This allows the infrastructure to be tested without full JIT compilation.
    //
    // In a full implementation, we would:
    // 1. Get the JIT context from thread-local storage
    // 2. Call compile_closure()
    // 3. Return the JitClosure on success
    //
    // For now, just return the original closure to indicate "not compiled"
    Ok(args[0].clone())
}

/// (jit-compiled? value) -> bool
///
/// Returns true if the value is a JIT-compiled closure.
pub fn prim_jit_compiled_p(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err(format!(
            "jit-compiled?: expected 1 argument, got {}",
            args.len()
        ));
    }

    Ok(Value::Bool(matches!(args[0], Value::JitClosure(_))))
}

/// (jit-compilable? closure) -> bool
///
/// Returns true if the closure can be JIT compiled.
pub fn prim_jit_compilable_p(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err(format!(
            "jit-compilable?: expected 1 argument, got {}",
            args.len()
        ));
    }

    match &args[0] {
        Value::Closure(c) => match &c.source_ast {
            Some(ast) => Ok(Value::Bool(is_jit_compilable(&ast.body))),
            None => Ok(Value::Bool(false)),
        },
        Value::JitClosure(_) => Ok(Value::Bool(true)), // Already compiled
        _ => Ok(Value::Bool(false)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jit_compiled_p_with_non_closure() {
        let result = prim_jit_compiled_p(&[Value::Int(42)]).unwrap();
        assert_eq!(result, Value::Bool(false));
    }

    #[test]
    fn test_jit_compiled_p_with_nil() {
        let result = prim_jit_compiled_p(&[Value::Nil]).unwrap();
        assert_eq!(result, Value::Bool(false));
    }

    #[test]
    fn test_jit_compilable_p_with_non_closure() {
        let result = prim_jit_compilable_p(&[Value::Int(42)]).unwrap();
        assert_eq!(result, Value::Bool(false));
    }

    #[test]
    fn test_jit_compilable_p_with_nil() {
        let result = prim_jit_compilable_p(&[Value::Nil]).unwrap();
        assert_eq!(result, Value::Bool(false));
    }

    #[test]
    fn test_jit_compile_with_non_closure() {
        let result = prim_jit_compile(&[Value::Int(42)]);
        assert!(result.is_err());
    }

    #[test]
    fn test_jit_compile_with_jit_closure() {
        // Create a JitClosure
        let jit_closure = JitClosure {
            code_ptr: std::ptr::null(),
            env: Rc::new(vec![]),
            arity: Arity::Exact(0),
            source: None,
            func_id: 1,
        };
        let value = Value::JitClosure(Rc::new(jit_closure));

        // jit-compile should return it as-is
        let result = prim_jit_compile(std::slice::from_ref(&value)).unwrap();
        assert!(matches!(result, Value::JitClosure(_)));
    }

    #[test]
    fn test_jit_compile_wrong_arg_count() {
        let result = prim_jit_compile(&[Value::Int(1), Value::Int(2)]);
        assert!(result.is_err());
    }

    #[test]
    fn test_jit_compilable_p_wrong_arg_count() {
        let result = prim_jit_compilable_p(&[Value::Int(1), Value::Int(2)]);
        assert!(result.is_err());
    }

    #[test]
    fn test_jit_compiled_p_wrong_arg_count() {
        let result = prim_jit_compiled_p(&[Value::Int(1), Value::Int(2)]);
        assert!(result.is_err());
    }
}
