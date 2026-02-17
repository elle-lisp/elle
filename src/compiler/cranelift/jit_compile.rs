//! On-demand JIT compilation for closures
//!
//! This module provides the `compile_closure` function that takes a Closure
//! with source AST and produces native code via Cranelift.

use super::context::JITContext;
use crate::compiler::ast::Expr;
use crate::compiler::cps::CpsJitCompiler;
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
        Expr::Var(_) => true,
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
        // Set is supported (mutation of local variables)
        Expr::Set { value, .. } => is_jit_compilable(value),
        // Cond is supported
        Expr::Cond { clauses, else_body } => {
            clauses
                .iter()
                .all(|(c, b)| is_jit_compilable(c) && is_jit_compilable(b))
                && else_body
                    .as_ref()
                    .map(|e| is_jit_compilable(e))
                    .unwrap_or(true)
        }
        // Check if Call is a supported primitive operation
        Expr::Call { func, args, .. } => is_jit_compilable_call(func, args),
        // These are NOT compilable yet:
        Expr::Lambda { .. } => false, // Nested lambdas need more work
        Expr::Letrec { .. } => false,
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
        Expr::Yield { .. } => false, // Yield requires CPS transformation
    }
}

/// Check if a Call expression can be JIT compiled
fn is_jit_compilable_call(func: &Expr, args: &[Expr]) -> bool {
    // The function must be a symbol (either Var(Global) or Literal(Symbol))
    let is_symbol_func = match func {
        Expr::Var(crate::binding::VarRef::Global { .. }) => true,
        Expr::Literal(v) => v.is_symbol(),
        _ => false,
    };

    if !is_symbol_func {
        return false;
    }

    // All arguments must be compilable
    if !args.iter().all(is_jit_compilable) {
        return false;
    }

    // We can't fully check the operator name without a symbol table,
    // but we trust that if it's a symbol and all args are compilable,
    // the compiler will try to handle it (either as a primitive or intrinsic).
    // The compiler will fail gracefully if the op isn't supported.
    // Note: We removed the args.len() restriction to allow primitives with any arity
    true
}

/// Compile a closure to native code
///
/// Returns CompileResult indicating success, not-compilable, or error.
/// Routes to CPS compilation for yielding closures, pure compilation otherwise.
pub fn compile_closure(
    closure: &Closure,
    jit_context: &Rc<RefCell<JITContext>>,
    symbols: &SymbolTable,
) -> CompileResult {
    // 1. Check if source AST is available
    let jit_lambda = match &closure.source_ast {
        Some(ast) => ast,
        None => return CompileResult::NotCompilable("No source AST available".to_string()),
    };

    // 2. Check if we should use CPS compilation based on effect
    if CpsJitCompiler::should_use_cps(closure.effect) {
        // CPS path - for closures that may yield
        // For now, fall through to pure compilation
        // TODO: Implement full CPS compilation path
        // return compile_cps_closure(closure, jit_context, symbols);
    }

    // 3. Check if body is compilable (pure path)
    if !is_jit_compilable(&jit_lambda.body) {
        return CompileResult::NotCompilable(
            "Closure body contains unsupported constructs".to_string(),
        );
    }

    // 4. Generate unique function name
    let func_id = next_func_id();
    let func_name = format!("jit_closure_{}", func_id);

    // 5. Compile the body
    // Convert captures from Vec<SymbolId> to Vec<(SymbolId, usize, usize)>
    let captures_tuple: Vec<(SymbolId, usize, usize)> = jit_lambda
        .captures
        .iter()
        .enumerate()
        .map(|(i, sym)| (*sym, 0, i))
        .collect();

    let code_ptr = match compile_lambda_body(
        jit_context,
        &func_name,
        &jit_lambda.params,
        &jit_lambda.body,
        &captures_tuple,
        symbols,
    ) {
        Ok(ptr) => ptr,
        Err(e) => return CompileResult::Error(e),
    };

    // 6. Create JitClosure
    // Convert environment from new Value to old Value
    let old_env: Vec<crate::value_old::Value> = closure
        .env
        .iter()
        .map(|v| crate::primitives::coroutines::new_value_to_old(*v))
        .collect();

    let jit_closure = JitClosure {
        code_ptr,
        env: Rc::new(old_env),
        arity: closure.arity,
        source: Some(Rc::new(closure.clone())),
        func_id,
        effect: closure.effect,
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
    symbols: &SymbolTable,
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

        // Create entry block with parameters (receives args_ptr, args_len, env_ptr)
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

        // Create body block with block parameters for each function argument
        // This enables tail call optimization by jumping to this block with new argument values
        let body_block = builder.create_block();
        for _ in 0..params.len() {
            builder.append_block_param(body_block, types::I64);
        }

        // In entry block: load arguments and jump to body block
        let mut arg_values = Vec::new();
        for (i, _sym_id) in params.iter().enumerate() {
            let offset = (i * 8) as i32;
            let arg_val = builder
                .ins()
                .load(types::I64, MemFlags::new(), args_ptr, offset);
            arg_values.push(arg_val);
        }
        builder.ins().jump(body_block, &arg_values);

        // Switch to body block and set up scope/stack
        builder.switch_to_block(body_block);
        // Don't seal body_block yet - it may have back-edges from self-recursive tail calls

        // Create compilation context with scope and stack management
        // NOTE: Do NOT push_scope() here - the AST expects depth=0 for the lambda's scope
        // (the converter uses depth=0 for the current lambda's parameters)
        let mut scope_manager = super::scoping::ScopeManager::new();
        let mut stack_allocator = super::stack_allocator::StackAllocator::new();

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

        // Bind parameters (they now come from body_block parameters)
        let body_block_params = builder.block_params(body_block).to_vec();
        for (i, sym_id) in params.iter().enumerate() {
            let (depth, index) = scope_manager.bind(*sym_id);
            let arg_val = body_block_params[i];

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
        let primitives = super::primitive_registry::PrimitiveRegistry::new();
        let mut compile_ctx = super::compiler::CompileContext::new(
            &mut builder,
            symbols,
            &mut ctx.module,
            &primitives,
        );
        compile_ctx.scope_manager = scope_manager;
        compile_ctx.stack_allocator = stack_allocator;

        // Set up function context for tail call optimization
        compile_ctx.func_ctx = Some(super::compiler::JitFunctionContext {
            body_block,
            current_function_name: Some(func_name.to_string()),
        });

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

        // Seal body_block after compilation (now all jumps to it are emitted)
        builder.seal_block(body_block);

        builder.finalize();
    }

    // Step 9-11: Define, finalize, and get code pointer
    unsafe {
        let ctx_ptr = jit_context.as_ptr();
        let ctx = &mut *ctx_ptr;

        // 9. Define the function in the module
        ctx.module
            .define_function(func_id, &mut ctx.ctx)
            .map_err(|e| format!("Failed to define function: {:?}", e))?;

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
        let expr = Expr::Literal(Value::int(42));
        assert!(is_jit_compilable(&expr));
    }

    #[test]
    fn test_is_jit_compilable_var() {
        let expr = Expr::Var(crate::binding::VarRef::local(0));
        assert!(is_jit_compilable(&expr));
    }

    #[test]
    fn test_is_jit_compilable_begin() {
        let expr = Expr::Begin(vec![
            Expr::Literal(Value::int(1)),
            Expr::Literal(Value::int(2)),
        ]);
        assert!(is_jit_compilable(&expr));
    }

    #[test]
    fn test_is_jit_compilable_if() {
        let expr = Expr::If {
            cond: Box::new(Expr::Literal(Value::bool(true))),
            then: Box::new(Expr::Literal(Value::int(1))),
            else_: Box::new(Expr::Literal(Value::int(2))),
        };
        assert!(is_jit_compilable(&expr));
    }

    #[test]
    fn test_is_jit_compilable_lambda_not_compilable() {
        let expr = Expr::Lambda {
            params: vec![],
            body: Box::new(Expr::Literal(Value::int(1))),
            captures: vec![],
            num_locals: 0,
            locals: vec![],
        };
        assert!(!is_jit_compilable(&expr));
    }

    #[test]
    fn test_is_jit_compilable_call_not_compilable() {
        let expr = Expr::Call {
            func: Box::new(Expr::Literal(Value::int(1))),
            args: vec![],
            tail: false,
        };
        assert!(!is_jit_compilable(&expr));
    }

    #[test]
    fn test_is_jit_compilable_and() {
        let expr = Expr::And(vec![
            Expr::Literal(Value::bool(true)),
            Expr::Literal(Value::bool(false)),
        ]);
        assert!(is_jit_compilable(&expr));
    }

    #[test]
    fn test_is_jit_compilable_or() {
        let expr = Expr::Or(vec![
            Expr::Literal(Value::bool(true)),
            Expr::Literal(Value::bool(false)),
        ]);
        assert!(is_jit_compilable(&expr));
    }

    #[test]
    fn test_is_jit_compilable_while() {
        let expr = Expr::While {
            cond: Box::new(Expr::Literal(Value::bool(true))),
            body: Box::new(Expr::Literal(Value::NIL)),
        };
        assert!(is_jit_compilable(&expr));
    }

    #[test]
    fn test_is_jit_compilable_nested_if() {
        let expr = Expr::If {
            cond: Box::new(Expr::Literal(Value::bool(true))),
            then: Box::new(Expr::If {
                cond: Box::new(Expr::Literal(Value::bool(false))),
                then: Box::new(Expr::Literal(Value::int(1))),
                else_: Box::new(Expr::Literal(Value::int(2))),
            }),
            else_: Box::new(Expr::Literal(Value::int(3))),
        };
        assert!(is_jit_compilable(&expr));
    }

    #[test]
    fn test_is_jit_compilable_set_is_compilable() {
        // Set expressions with compilable values are now compilable
        let expr = Expr::Set {
            target: crate::binding::VarRef::local(0),
            value: Box::new(Expr::Literal(Value::int(1))),
        };
        assert!(is_jit_compilable(&expr));
    }

    #[test]
    fn test_is_jit_compilable_letrec_not_compilable() {
        let expr = Expr::Letrec {
            bindings: vec![(SymbolId(1), Expr::Literal(Value::int(1)))],
            body: Box::new(Expr::Literal(Value::int(2))),
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
            effect: crate::compiler::effects::Effect::Pure,
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

        let body = Expr::Literal(Value::int(42));
        let params = vec![];
        let captures = vec![];
        let symbols = crate::symbol::SymbolTable::new();

        let result = compile_lambda_body(
            &jit_context,
            "test_literal",
            &params,
            &body,
            &captures,
            &symbols,
        );
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
        // Note: index=0 because parameters are in the lambda's base scope
        let body = Expr::Var(crate::binding::VarRef::local(0));
        let params = vec![SymbolId(1)];
        let captures = vec![];
        let symbols = crate::symbol::SymbolTable::new();

        let result = compile_lambda_body(
            &jit_context,
            "test_param",
            &params,
            &body,
            &captures,
            &symbols,
        );
        assert!(result.is_ok(), "Failed to compile lambda: {:?}", result);
        assert!(
            !result.unwrap().is_null(),
            "Code pointer should not be null"
        );
    }

    #[test]
    fn test_is_jit_compilable_primitive_call() {
        // Test that calls to primitives are now compilable
        let mut symbols = crate::symbol::SymbolTable::new();
        let first_sym = symbols.intern("first");

        let expr = Expr::Call {
            func: Box::new(Expr::Var(crate::binding::VarRef::global(first_sym))),
            args: vec![Expr::Literal(Value::NIL)],
            tail: false,
        };

        // This should now be compilable (previously would have failed due to arity check)
        assert!(
            is_jit_compilable(&expr),
            "Primitive call should be compilable"
        );
    }

    #[test]
    fn test_is_jit_compilable_multi_arg_primitive() {
        // Test that calls to multi-arg primitives are compilable
        let mut symbols = crate::symbol::SymbolTable::new();
        let cons_sym = symbols.intern("cons");

        let expr = Expr::Call {
            func: Box::new(Expr::Var(crate::binding::VarRef::global(cons_sym))),
            args: vec![Expr::Literal(Value::int(1)), Expr::Literal(Value::NIL)],
            tail: false,
        };

        // This should be compilable
        assert!(
            is_jit_compilable(&expr),
            "Multi-arg primitive call should be compilable"
        );
    }
}
