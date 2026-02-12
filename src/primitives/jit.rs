//! JIT compilation primitives

use crate::compiler::cranelift::context::JITContext;
use crate::compiler::cranelift::jit_compile::{compile_closure, is_jit_compilable, CompileResult};
use crate::symbol::SymbolTable;
use crate::value::{TableKey, Value};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(test)]
use crate::value::{Arity, JitClosure};

thread_local! {
    /// Thread-local storage for JIT context
    static JIT_CONTEXT: RefCell<Option<Rc<RefCell<JITContext>>>> = const { RefCell::new(None) };
    /// Thread-local storage for symbol table
    static SYMBOL_TABLE: RefCell<Option<*mut SymbolTable>> = const { RefCell::new(None) };
}

/// Statistics counters for JIT compilation (global, shared across threads)
static COMPILED_FUNCTIONS: AtomicU64 = AtomicU64::new(0);
static TOTAL_COMPILATIONS: AtomicU64 = AtomicU64::new(0);
static FAILED_COMPILATIONS: AtomicU64 = AtomicU64::new(0);

/// Initialize the JIT context for primitives
///
/// Creates a new JIT context if one doesn't exist.
/// Must be called before using jit-compile.
pub fn init_jit_context() {
    JIT_CONTEXT.with(|ctx| {
        let mut ctx = ctx.borrow_mut();
        if ctx.is_none() {
            if let Ok(jit_ctx) = JITContext::new() {
                *ctx = Some(Rc::new(RefCell::new(jit_ctx)));
            }
        }
    });
}

/// Set the symbol table context for JIT primitives
///
/// # Safety
/// The pointer must remain valid for the duration of use.
pub fn set_jit_symbol_table(symbols: *mut SymbolTable) {
    SYMBOL_TABLE.with(|st| {
        *st.borrow_mut() = Some(symbols);
    });
}

/// Clear the JIT context
pub fn clear_jit_context() {
    JIT_CONTEXT.with(|ctx| {
        *ctx.borrow_mut() = None;
    });
    SYMBOL_TABLE.with(|st| {
        *st.borrow_mut() = None;
    });
}

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

    // Try to compile using thread-local JIT context
    let result = JIT_CONTEXT.with(|ctx_cell| {
        let ctx_opt = ctx_cell.borrow();
        match &*ctx_opt {
            Some(jit_ctx) => {
                // Get symbol table
                SYMBOL_TABLE.with(|st_cell| {
                    let st_ptr = st_cell.borrow();
                    match *st_ptr {
                        Some(symbols_ptr) => {
                            // SAFETY: Caller ensures pointer validity via set_jit_symbol_table
                            let symbols = unsafe { &*symbols_ptr };
                            TOTAL_COMPILATIONS.fetch_add(1, Ordering::Relaxed);
                            Some(compile_closure(closure, jit_ctx, symbols))
                        }
                        None => {
                            // No symbol table, try with empty one
                            let symbols = SymbolTable::new();
                            TOTAL_COMPILATIONS.fetch_add(1, Ordering::Relaxed);
                            Some(compile_closure(closure, jit_ctx, &symbols))
                        }
                    }
                })
            }
            None => None,
        }
    });

    match result {
        Some(CompileResult::Success(jit_closure)) => {
            COMPILED_FUNCTIONS.fetch_add(1, Ordering::Relaxed);
            Ok(Value::JitClosure(Rc::new(jit_closure)))
        }
        Some(CompileResult::NotCompilable(_reason)) => {
            // Not compilable, return original closure silently
            Ok(args[0].clone())
        }
        Some(CompileResult::Error(e)) => {
            FAILED_COMPILATIONS.fetch_add(1, Ordering::Relaxed);
            // Compilation error - still return original closure for graceful degradation
            eprintln!("JIT compilation failed: {}", e);
            Ok(args[0].clone())
        }
        None => {
            // No JIT context available, return original closure
            Ok(args[0].clone())
        }
    }
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

/// (jit-stats) -> struct
///
/// Returns statistics about JIT compilation.
/// Returns a struct with the following fields:
/// - compiled-functions: Number of functions compiled to native code
/// - jit-enabled: Whether JIT compilation is available
pub fn prim_jit_stats(args: &[Value]) -> Result<Value, String> {
    if !args.is_empty() {
        return Err(format!(
            "jit-stats: expected 0 arguments, got {}",
            args.len()
        ));
    }

    let mut stats = BTreeMap::new();

    // Check if JIT context is available
    let jit_enabled = JIT_CONTEXT.with(|ctx| ctx.borrow().is_some());

    // Get actual statistics from atomic counters
    let compiled = COMPILED_FUNCTIONS.load(Ordering::Relaxed) as i64;
    let total = TOTAL_COMPILATIONS.load(Ordering::Relaxed) as i64;
    let failed = FAILED_COMPILATIONS.load(Ordering::Relaxed) as i64;

    stats.insert(
        TableKey::String("compiled-functions".to_string()),
        Value::Int(compiled),
    );
    stats.insert(
        TableKey::String("total-compilations".to_string()),
        Value::Int(total),
    );
    stats.insert(
        TableKey::String("failed-compilations".to_string()),
        Value::Int(failed),
    );
    // These could be tracked with more infrastructure
    stats.insert(TableKey::String("cache-hits".to_string()), Value::Int(0));
    stats.insert(TableKey::String("cache-misses".to_string()), Value::Int(0));
    stats.insert(TableKey::String("hot-closures".to_string()), Value::Int(0));
    stats.insert(
        TableKey::String("native-code-bytes".to_string()),
        Value::Int(0),
    );
    stats.insert(
        TableKey::String("compilation-time-ms".to_string()),
        Value::Int(0),
    );
    stats.insert(
        TableKey::String("jit-enabled".to_string()),
        Value::Bool(jit_enabled),
    );

    // Return as immutable struct
    Ok(Value::Struct(Rc::new(stats)))
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

    #[test]
    fn test_jit_stats_returns_struct() {
        let result = prim_jit_stats(&[]).unwrap();
        assert!(matches!(result, Value::Struct(_)));
    }

    #[test]
    fn test_jit_stats_no_args() {
        let result = prim_jit_stats(&[Value::Int(1)]);
        assert!(result.is_err());
    }

    #[test]
    fn test_jit_stats_has_expected_fields() {
        let result = prim_jit_stats(&[]).unwrap();
        if let Value::Struct(s) = result {
            assert!(s.contains_key(&TableKey::String("compiled-functions".to_string())));
            assert!(s.contains_key(&TableKey::String("total-compilations".to_string())));
            assert!(s.contains_key(&TableKey::String("failed-compilations".to_string())));
            assert!(s.contains_key(&TableKey::String("cache-hits".to_string())));
            assert!(s.contains_key(&TableKey::String("cache-misses".to_string())));
            assert!(s.contains_key(&TableKey::String("hot-closures".to_string())));
            assert!(s.contains_key(&TableKey::String("native-code-bytes".to_string())));
            assert!(s.contains_key(&TableKey::String("compilation-time-ms".to_string())));
            assert!(s.contains_key(&TableKey::String("jit-enabled".to_string())));
        } else {
            panic!("Expected struct");
        }
    }

    #[test]
    fn test_jit_stats_jit_enabled_is_bool() {
        let result = prim_jit_stats(&[]).unwrap();
        if let Value::Struct(s) = result {
            let jit_enabled = s.get(&TableKey::String("jit-enabled".to_string()));
            // jit-enabled is a bool (may be true or false depending on context initialization)
            assert!(matches!(jit_enabled, Some(Value::Bool(_))));
        } else {
            panic!("Expected struct");
        }
    }

    #[test]
    fn test_jit_stats_compiled_functions_is_int() {
        let result = prim_jit_stats(&[]).unwrap();
        if let Value::Struct(s) = result {
            let compiled = s.get(&TableKey::String("compiled-functions".to_string()));
            assert!(matches!(compiled, Some(Value::Int(0))));
        } else {
            panic!("Expected struct");
        }
    }
}
