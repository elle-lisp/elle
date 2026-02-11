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
/// Creates a Cranelift function with signature:
/// fn(args_ptr: i64, args_len: i64, env_ptr: i64) -> i64
///
/// Where:
/// - args_ptr: pointer to array of argument values (as i64)
/// - args_len: number of arguments
/// - env_ptr: pointer to array of captured values (as i64)
/// - return: encoded result value (as i64)
fn compile_lambda_body(
    jit_context: &Rc<RefCell<JITContext>>,
    func_name: &str,
    params: &[SymbolId],
    body: &Expr,
    captures: &[(SymbolId, usize, usize)],
) -> Result<*const u8, String> {
    use cranelift::prelude::*;
    use cranelift_module::Module;

    // Step 1-3: Create function signature and declare it
    let func_id = {
        let mut ctx = jit_context.borrow_mut();

        // 1. Create function signature: fn(args_ptr, args_len, env_ptr) -> i64
        let mut sig = ctx.module.make_signature();
        sig.params.push(AbiParam::new(types::I64)); // args_ptr
        sig.params.push(AbiParam::new(types::I64)); // args_len
        sig.params.push(AbiParam::new(types::I64)); // env_ptr
        sig.returns.push(AbiParam::new(types::I64)); // return value

        // 2. Declare the function
        let func_id = ctx
            .module
            .declare_function(func_name, cranelift_module::Linkage::Local, &sig)
            .map_err(|e| format!("Failed to declare function: {}", e))?;

        // 3. Set up function context
        ctx.ctx.func.signature = sig.clone();

        func_id
    };

    // Step 4-8: Build the function body
    // We need to be careful with borrows here - FunctionBuilder borrows ctx.ctx.func and ctx.builder_ctx
    // while CompileContext borrows ctx.module. We'll use unsafe to work around the borrow checker.
    unsafe {
        let ctx_ptr = jit_context.as_ptr();
        let ctx = &mut *ctx_ptr;

        let mut builder = FunctionBuilder::new(&mut ctx.ctx.func, &mut ctx.builder_ctx);

        // Create entry block with parameters
        let entry_block = builder.create_block();
        builder.append_block_param(entry_block, types::I64); // args_ptr
        builder.append_block_param(entry_block, types::I64); // args_len
        builder.append_block_param(entry_block, types::I64); // env_ptr
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);

        // Get the function parameters
        let block_params = builder.block_params(entry_block).to_vec();
        let args_ptr = block_params[0];
        let _args_len = block_params[1];
        let env_ptr = block_params[2];

        // Create compilation context with scope and stack management
        let mut scope_manager = super::scoping::ScopeManager::new();
        let mut stack_allocator = super::stack_allocator::StackAllocator::new();

        scope_manager.push_scope();

        // Bind captures first (they come from env_ptr)
        for (i, (sym_id, _, _)) in captures.iter().enumerate() {
            let (depth, index) = scope_manager.bind(*sym_id);
            let offset = (i * 8) as i32;

            // Load capture from env array: env_ptr[i]
            let cap_val = builder
                .ins()
                .load(types::I64, MemFlags::new(), env_ptr, offset);

            // Allocate stack slot and store
            let (slot, _) = stack_allocator.allocate(
                &mut builder,
                depth,
                index,
                super::stack_allocator::SlotType::I64,
            )?;
            builder.ins().stack_store(cap_val, slot, 0);
        }

        // Bind parameters (they come from args_ptr)
        for (i, sym_id) in params.iter().enumerate() {
            let (depth, index) = scope_manager.bind(*sym_id);
            let offset = (i * 8) as i32;

            // Load param from args array: args_ptr[i]
            let arg_val = builder
                .ins()
                .load(types::I64, MemFlags::new(), args_ptr, offset);

            // Allocate stack slot and store
            let (slot, _) = stack_allocator.allocate(
                &mut builder,
                depth,
                index,
                super::stack_allocator::SlotType::I64,
            )?;
            builder.ins().stack_store(arg_val, slot, 0);
        }

        // Compile the body expression
        let symbols = crate::symbol::SymbolTable::new();
        let mut compile_ctx =
            super::compiler::CompileContext::new(&mut builder, &symbols, &mut ctx.module);
        compile_ctx.scope_manager = scope_manager;
        compile_ctx.stack_allocator = stack_allocator;

        let result = super::compiler::ExprCompiler::compile_expr_block(&mut compile_ctx, body)?;

        // Return the result
        let return_val = match result {
            super::compiler::IrValue::I64(v) => v,
            super::compiler::IrValue::F64(_v) => {
                // TODO: Encode float as its bit representation (i64)
                // For now, return 0
                compile_ctx.builder.ins().iconst(types::I64, 0)
            }
        };
        compile_ctx.builder.ins().return_(&[return_val]);

        // Drop compile_ctx to release the borrow on builder
        drop(compile_ctx);
        builder.finalize();
    }

    // Step 9-11: Define, finalize, and get code pointer
    unsafe {
        let ctx_ptr = jit_context.as_ptr();
        let ctx = &mut *ctx_ptr;

        // 9. Define the function in the module
        ctx.module
            .define_function(func_id, &mut ctx.ctx)
            .map_err(|e| format!("Failed to define function: {}", e))?;

        // 10. Clear context for next use
        ctx.ctx.clear();

        // 11. Finalize and get code pointer
        ctx.module
            .finalize_definitions()
            .map_err(|e| format!("Failed to finalize: {}", e))?;

        let code_ptr = ctx.module.get_finalized_function(func_id);

        // Store in functions map for tracking
        ctx.functions.insert(func_name.to_string(), code_ptr);

        Ok(code_ptr)
    }
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

    #[test]
    fn test_compile_lambda_body_simple_literal() {
        // Test compiling a simple lambda that returns a literal
        use std::cell::RefCell;
        use std::rc::Rc;

        let jit_context = Rc::new(RefCell::new(
            super::super::context::JITContext::new().expect("Failed to create JIT context"),
        ));

        let body = Expr::Literal(Value::Int(42));
        let params = vec![];
        let captures = vec![];

        let result = compile_lambda_body(&jit_context, "test_literal", &params, &body, &captures);
        assert!(result.is_ok(), "Failed to compile lambda: {:?}", result);
        assert!(
            !result.unwrap().is_null(),
            "Code pointer should not be null"
        );
    }

    #[test]
    fn test_compile_lambda_body_with_parameter() {
        // Test compiling a lambda that uses a parameter
        use std::cell::RefCell;
        use std::rc::Rc;

        let jit_context = Rc::new(RefCell::new(
            super::super::context::JITContext::new().expect("Failed to create JIT context"),
        ));

        // (fn (x) x) - identity function
        let body = Expr::Var(SymbolId(1), 1, 0);
        let params = vec![SymbolId(1)];
        let captures = vec![];

        let result = compile_lambda_body(&jit_context, "test_param", &params, &body, &captures);
        assert!(result.is_ok(), "Failed to compile lambda: {:?}", result);
        assert!(
            !result.unwrap().is_null(),
            "Code pointer should not be null"
        );
    }
}
