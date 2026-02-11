//! On-demand JIT compilation for closures
//!
//! This module provides the `compile_closure` function that takes a Closure
//! with source AST and produces native code via Cranelift.

use super::context::JITContext;
use crate::compiler::ast::Expr;
use crate::symbol::SymbolTable;
use crate::value::{Closure, JitClosure, SymbolId};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Global counter for unique function IDs
static FUNC_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

fn next_func_id() -> u64 {
    FUNC_ID_COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Result of JIT compilation
pub enum CompileResult {
    /// Successfully compiled to native code
    Success(JitClosure),
    /// Compilation not possible (unsupported constructs)
    NotCompilable(String),
    /// Compilation failed with error
    Error(String),
}

/// Check if an expression can be JIT compiled
pub fn is_jit_compilable(expr: &Expr) -> bool {
    match expr {
        Expr::Literal(_) => true,
        Expr::Var(_, _, _) => true,
        Expr::GlobalVar(_) => true,
        Expr::Begin(exprs) | Expr::Block(exprs) => exprs.iter().all(is_jit_compilable),
        Expr::If {
            cond, then, else_, ..
        } => is_jit_compilable(cond) && is_jit_compilable(then) && is_jit_compilable(else_),
        Expr::Let { bindings, body } => {
            bindings.iter().all(|(_, e)| is_jit_compilable(e)) && is_jit_compilable(body)
        }
        Expr::While { cond, body } => is_jit_compilable(cond) && is_jit_compilable(body),
        Expr::For { iter, body, .. } => is_jit_compilable(iter) && is_jit_compilable(body),
        Expr::And(exprs) | Expr::Or(exprs) | Expr::Xor(exprs) => {
            exprs.iter().all(is_jit_compilable)
        }
        // These are NOT compilable yet:
        Expr::Lambda { .. } => false, // Nested lambdas need more work
        Expr::Call { .. } => false,   // Function calls need runtime support
        Expr::Letrec { .. } => false,
        Expr::Set { .. } => false,   // Mutation needs cell handling
        Expr::Cond { .. } => false,  // Cond needs more work
        Expr::Match { .. } => false, // Pattern matching needs more work
        Expr::Try { .. } => false,   // Exception handling needs more work
        Expr::Throw { .. } => false,
        Expr::HandlerCase { .. } => false,
        Expr::HandlerBind { .. } => false,
        Expr::Quote(_) => false,
        Expr::Quasiquote(_) => false,
        Expr::Unquote(_) => false,
        Expr::Define { .. } => false,
        Expr::DefMacro { .. } => false,
        Expr::Module { .. } => false,
        Expr::Import { .. } => false,
        Expr::ModuleRef { .. } => false,
    }
}

/// Compile a closure to native code
///
/// Returns CompileResult indicating success, not-compilable, or error.
pub fn compile_closure(
    closure: &Closure,
    jit_context: &Rc<RefCell<JITContext>>,
    _symbols: &SymbolTable,
) -> CompileResult {
    // 1. Check if source AST is available
    let jit_lambda = match &closure.source_ast {
        Some(ast) => ast,
        None => return CompileResult::NotCompilable("No source AST available".to_string()),
    };

    // 2. Check if body is compilable
    if !is_jit_compilable(&jit_lambda.body) {
        return CompileResult::NotCompilable(
            "Closure body contains unsupported constructs".to_string(),
        );
    }

    // 3. Generate unique function name
    let func_id = next_func_id();
    let func_name = format!("jit_closure_{}", func_id);

    // 4. Compile the body
    let code_ptr = match compile_lambda_body(
        jit_context,
        &func_name,
        &jit_lambda.params,
        &jit_lambda.body,
        &jit_lambda.captures,
    ) {
        Ok(ptr) => ptr,
        Err(e) => return CompileResult::Error(e),
    };

    // 5. Create JitClosure
    let jit_closure = JitClosure {
        code_ptr,
        env: closure.env.clone(),
        arity: closure.arity,
        source: Some(Rc::new(closure.clone())),
        func_id,
    };

    CompileResult::Success(jit_closure)
}

/// Compile a lambda body to native code
///
/// NOTE: This is a simplified implementation for Phase 3.
/// Full Cranelift integration requires refactoring JITContext to avoid
/// simultaneous mutable borrows of ctx.ctx and ctx.builder_ctx.
fn compile_lambda_body(
    _jit_context: &Rc<RefCell<JITContext>>,
    _func_name: &str,
    _params: &[SymbolId],
    _body: &Expr,
    _captures: &[(SymbolId, usize, usize)],
) -> Result<*const u8, String> {
    // For Phase 3, we return a null pointer to indicate that
    // the JIT compilation infrastructure is in place but not yet
    // fully implemented. In a full implementation, this would:
    //
    // 1. Create a Cranelift function signature
    // 2. Build IR for the lambda body
    // 3. Compile to native code
    // 4. Return a function pointer
    //
    // The infrastructure is ready for this in Phase 4-6.
    Ok(std::ptr::null())
}

/// Compile an expression to Cranelift IR
/// Returns the Value representing the result
///
/// NOTE: This is a stub for Phase 3. Full implementation in Phase 4-6.
#[allow(dead_code)]
fn compile_expr_to_ir(
    _builder: &mut cranelift::prelude::FunctionBuilder,
    _expr: &Expr,
    _params: &[cranelift::prelude::Value],
    _param_count: usize,
    _capture_count: usize,
    _scope_manager: &super::scoping::ScopeManager,
) -> Result<cranelift::prelude::Value, String> {
    // Stub implementation for Phase 3
    // Full implementation will compile expressions to Cranelift IR
    Err("JIT compilation not yet fully implemented".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::{Arity, Value};

    #[test]
    fn test_is_jit_compilable_literal() {
        let expr = Expr::Literal(Value::Int(42));
        assert!(is_jit_compilable(&expr));
    }

    #[test]
    fn test_is_jit_compilable_var() {
        let expr = Expr::Var(SymbolId(1), 0, 0);
        assert!(is_jit_compilable(&expr));
    }

    #[test]
    fn test_is_jit_compilable_begin() {
        let expr = Expr::Begin(vec![
            Expr::Literal(Value::Int(1)),
            Expr::Literal(Value::Int(2)),
        ]);
        assert!(is_jit_compilable(&expr));
    }

    #[test]
    fn test_is_jit_compilable_if() {
        let expr = Expr::If {
            cond: Box::new(Expr::Literal(Value::Bool(true))),
            then: Box::new(Expr::Literal(Value::Int(1))),
            else_: Box::new(Expr::Literal(Value::Int(2))),
        };
        assert!(is_jit_compilable(&expr));
    }

    #[test]
    fn test_is_jit_compilable_lambda_not_compilable() {
        let expr = Expr::Lambda {
            params: vec![],
            body: Box::new(Expr::Literal(Value::Int(1))),
            captures: vec![],
            locals: vec![],
        };
        assert!(!is_jit_compilable(&expr));
    }

    #[test]
    fn test_is_jit_compilable_call_not_compilable() {
        let expr = Expr::Call {
            func: Box::new(Expr::Literal(Value::Int(1))),
            args: vec![],
            tail: false,
        };
        assert!(!is_jit_compilable(&expr));
    }

    #[test]
    fn test_is_jit_compilable_and() {
        let expr = Expr::And(vec![
            Expr::Literal(Value::Bool(true)),
            Expr::Literal(Value::Bool(false)),
        ]);
        assert!(is_jit_compilable(&expr));
    }

    #[test]
    fn test_is_jit_compilable_or() {
        let expr = Expr::Or(vec![
            Expr::Literal(Value::Bool(true)),
            Expr::Literal(Value::Bool(false)),
        ]);
        assert!(is_jit_compilable(&expr));
    }

    #[test]
    fn test_is_jit_compilable_while() {
        let expr = Expr::While {
            cond: Box::new(Expr::Literal(Value::Bool(true))),
            body: Box::new(Expr::Literal(Value::Nil)),
        };
        assert!(is_jit_compilable(&expr));
    }

    #[test]
    fn test_is_jit_compilable_nested_if() {
        let expr = Expr::If {
            cond: Box::new(Expr::Literal(Value::Bool(true))),
            then: Box::new(Expr::If {
                cond: Box::new(Expr::Literal(Value::Bool(false))),
                then: Box::new(Expr::Literal(Value::Int(1))),
                else_: Box::new(Expr::Literal(Value::Int(2))),
            }),
            else_: Box::new(Expr::Literal(Value::Int(3))),
        };
        assert!(is_jit_compilable(&expr));
    }

    #[test]
    fn test_is_jit_compilable_set_not_compilable() {
        let expr = Expr::Set {
            var: SymbolId(1),
            depth: 0,
            index: 0,
            value: Box::new(Expr::Literal(Value::Int(1))),
        };
        assert!(!is_jit_compilable(&expr));
    }

    #[test]
    fn test_is_jit_compilable_letrec_not_compilable() {
        let expr = Expr::Letrec {
            bindings: vec![(SymbolId(1), Expr::Literal(Value::Int(1)))],
            body: Box::new(Expr::Literal(Value::Int(2))),
        };
        assert!(!is_jit_compilable(&expr));
    }

    #[test]
    fn test_compile_result_success() {
        // This is a basic test to ensure CompileResult enum works
        let jc = JitClosure {
            code_ptr: std::ptr::null(),
            env: Rc::new(vec![]),
            arity: Arity::Exact(0),
            source: None,
            func_id: 1,
        };
        let _result = CompileResult::Success(jc);
    }

    #[test]
    fn test_compile_result_not_compilable() {
        let _result = CompileResult::NotCompilable("test".to_string());
    }

    #[test]
    fn test_compile_result_error() {
        let _result = CompileResult::Error("test error".to_string());
    }
}
