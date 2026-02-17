// JIT Executor
//
// Executes JIT-compiled code by interfacing with Cranelift infrastructure.
// This bridges the gap between compile-time JIT compilation and runtime execution.

use super::ast::Expr;
use super::cranelift::compiler::ExprCompiler;
use super::cranelift::context::JITContext;
use crate::symbol::SymbolTable;
use crate::value::Value;
use std::cell::RefCell;
use std::rc::Rc;

/// A compiled and cached native code function
#[derive(Clone)]
pub struct JitCompiledCode {
    /// Function pointer to native code
    func_ptr: *const u8,
    /// Expression hash for validation (used for cache validation)
    #[allow(dead_code)]
    expr_hash: u64,
}

impl JitCompiledCode {
    /// Create a new compiled code entry
    pub fn new(func_ptr: *const u8, expr_hash: u64) -> Self {
        JitCompiledCode {
            func_ptr,
            expr_hash,
        }
    }

    /// Get the function pointer
    pub fn get_ptr(&self) -> *const u8 {
        self.func_ptr
    }
}

/// A cached JIT compilation result
#[derive(Clone)]
pub struct CachedJitCode {
    /// Expression signature (for matching)
    pub expr_hash: u64,
    /// Whether this was successfully compiled
    pub compiled: bool,
    /// The compiled native code if successful
    pub native_code: Option<JitCompiledCode>,
}

impl CachedJitCode {
    /// Create a new cached JIT code entry
    pub fn new(expr_hash: u64, compiled: bool) -> Self {
        CachedJitCode {
            expr_hash,
            compiled,
            native_code: None,
        }
    }

    /// Create a successful compilation cache entry with native code
    pub fn with_native(expr_hash: u64, func_ptr: *const u8) -> Self {
        CachedJitCode {
            expr_hash,
            compiled: true,
            native_code: Some(JitCompiledCode::new(func_ptr, expr_hash)),
        }
    }

    /// Compute hash of an expression for caching
    pub fn compute_hash(expr: &Expr) -> u64 {
        // Simple hash: use memory address as proxy for now
        // In production, would use proper hash of expression structure
        expr as *const Expr as u64
    }
}

/// JIT Code Executor manages execution of JIT-compiled code
pub struct JitExecutor {
    /// Cache of compiled code (RefCell for interior mutability in single-threaded context)
    cache: Rc<RefCell<std::collections::HashMap<u64, CachedJitCode>>>,
    /// JIT context for compilation
    jit_context: Option<Rc<RefCell<JITContext>>>,
    /// Call counters for profiling hot functions (bytecode pointer -> call count)
    call_counts: Rc<RefCell<std::collections::HashMap<*const u8, usize>>>,
}

impl JitExecutor {
    /// Create a new JIT executor
    pub fn new() -> Result<Self, String> {
        let jit_ctx = JITContext::new().ok();
        Ok(JitExecutor {
            cache: Rc::new(RefCell::new(std::collections::HashMap::new())),
            jit_context: jit_ctx.map(|ctx| Rc::new(RefCell::new(ctx))),
            call_counts: Rc::new(RefCell::new(std::collections::HashMap::new())),
        })
    }

    /// Try to JIT compile and execute an expression
    pub fn try_jit_execute(
        &mut self,
        expr: &Expr,
        symbols: &SymbolTable,
    ) -> Result<Option<Value>, String> {
        if self.jit_context.is_none() {
            return Ok(None);
        }

        // Check cache first
        let hash = CachedJitCode::compute_hash(expr);

        {
            let cache = self.cache.borrow();
            if let Some(cached) = cache.get(&hash) {
                if !cached.compiled {
                    // Already tried and failed
                    return Ok(None);
                }
                // If we have native code, try to execute it
                if let Some(native) = &cached.native_code {
                    return self.execute_native_code(native, expr);
                }
            }
        }

        // Try to compile with Cranelift
        let result = match expr {
            // Literals can be compiled and executed immediately
            Expr::Literal(val) => {
                let mut cache = self.cache.borrow_mut();
                cache.insert(hash, CachedJitCode::new(hash, true));
                Ok(Some(*val))
            }

            // If expressions and Begin can be JIT compiled
            Expr::If { .. } | Expr::Begin(_) => self.compile_and_execute_expr(expr, hash, symbols),

            // Call expressions - try to compile them
            Expr::Call { .. } => self.compile_and_execute_expr(expr, hash, symbols),

            // Everything else
            _ => {
                let mut cache = self.cache.borrow_mut();
                cache.insert(hash, CachedJitCode::new(hash, false));
                Ok(None)
            }
        };

        result
    }

    /// Compile and execute an expression with Cranelift
    fn compile_and_execute_expr(
        &mut self,
        expr: &Expr,
        hash: u64,
        symbols: &SymbolTable,
    ) -> Result<Option<Value>, String> {
        let ctx = self
            .jit_context
            .as_ref()
            .ok_or("JIT context not available")?;
        let mut jit_ctx = ctx.borrow_mut();

        // Generate a unique function name
        let func_name = format!("jit_expr_{:x}", hash);

        // Compile the expression to native code
        match ExprCompiler::compile_expr(&mut jit_ctx, &func_name, expr, symbols) {
            Ok(func_ptr) => {
                // Cache the compiled code
                let mut cache = self.cache.borrow_mut();
                cache.insert(hash, CachedJitCode::with_native(hash, func_ptr));

                drop(cache); // Release borrow before executing
                drop(jit_ctx); // Release JIT context borrow

                // Execute the native code
                let compiled = JitCompiledCode::new(func_ptr, hash);
                self.execute_native_code(&compiled, expr)
            }
            Err(_e) => {
                // Compilation failed, mark as unsuccessful
                let mut cache = self.cache.borrow_mut();
                cache.insert(hash, CachedJitCode::new(hash, false));
                Ok(None)
            }
        }
    }

    /// Execute compiled native code and convert result back to Elle Value
    fn execute_native_code(
        &self,
        compiled: &JitCompiledCode,
        expr: &Expr,
    ) -> Result<Option<Value>, String> {
        unsafe {
            // Cast function pointer to a function that takes two i64 arguments and returns i64
            // The compiled function signature: fn(args_ptr: i64, args_len: i64) -> i64
            let func: extern "C" fn(i64, i64) -> i64 = std::mem::transmute(compiled.get_ptr());

            // Call the native code with null args (we don't have args for simple expressions)
            let result = func(0, 0);

            // Decode the result back to Elle Value
            let value = Self::decode_native_result(result, expr)?;
            Ok(Some(value))
        }
    }

    /// Decode a native code result (i64) back to an Elle Value
    fn decode_native_result(result: i64, expr: &Expr) -> Result<Value, String> {
        // For now, we need to know what type of value was compiled
        // In the future, we'd track type information through compilation
        match expr {
            Expr::Literal(val) => {
                if val.is_nil() || val.is_empty_list() {
                    Ok(Value::NIL)
                } else if val.is_bool() {
                    // Booleans are encoded as 0 (false) or 1 (true)
                    Ok(Value::bool(result != 0))
                } else if val.is_int() {
                    // Integers are stored directly
                    Ok(Value::int(result))
                } else if val.is_float() {
                    // For now, we can't properly decode floats without more context
                    // The result would be bit-encoded, but we can't decode here
                    Err("Float results not yet supported in JIT execution".to_string())
                } else {
                    Err(format!(
                        "Cannot decode native result for literal: {:?}",
                        val
                    ))
                }
            }
            Expr::If { .. } | Expr::Begin(_) => {
                // For conditionals and sequences, try to infer from structure
                // For now, treat as integer
                Ok(Value::int(result))
            }
            _ => {
                // Default: treat as integer
                Ok(Value::int(result))
            }
        }
    }

    /// Check if JIT context is available
    pub fn has_jit_context(&self) -> bool {
        self.jit_context.is_some()
    }

    /// Get cache statistics
    pub fn cache_stats(&self) -> (usize, usize) {
        let cache = self.cache.borrow();
        let total = cache.len();
        let compiled = cache.values().filter(|c| c.compiled).count();
        (compiled, total)
    }

    /// Try to compile and cache bytecode for a closure (optimization attempt)
    /// Returns true if compilation succeeded and code can be used
    pub fn can_compile_bytecode(&self, _bytecode: &[u8]) -> bool {
        // Check if we can compile this bytecode signature
        // For now, return false - full implementation would analyze bytecode pattern
        // This is a placeholder for future bytecode analysis
        false
    }

    /// Get cache statistics for profiling
    pub fn get_cache_size(&self) -> usize {
        self.cache.borrow().len()
    }

    /// Record a closure call and return whether it's "hot" (called 10+ times)
    pub fn record_closure_call(&self, bytecode_ptr: *const u8) -> bool {
        let mut counts = self.call_counts.borrow_mut();
        let count = counts.entry(bytecode_ptr).or_insert(0);
        *count += 1;
        *count >= 10
    }

    /// Get the call count for a closure
    pub fn get_call_count(&self, bytecode_ptr: *const u8) -> usize {
        self.call_counts
            .borrow()
            .get(&bytecode_ptr)
            .copied()
            .unwrap_or(0)
    }
}

impl Default for JitExecutor {
    fn default() -> Self {
        Self::new().unwrap_or(JitExecutor {
            cache: Rc::new(RefCell::new(std::collections::HashMap::new())),
            jit_context: None,
            call_counts: Rc::new(RefCell::new(std::collections::HashMap::new())),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jit_executor_creation() {
        let executor = JitExecutor::new();
        assert!(executor.is_ok());
    }

    #[test]
    fn test_jit_executor_literal_execution() {
        let mut executor = JitExecutor::new().unwrap();
        let symbols = SymbolTable::new();
        let expr = Expr::Literal(Value::int(42));

        let result = executor.try_jit_execute(&expr, &symbols);
        assert!(result.is_ok());
        // Literals should execute successfully
        let res = result.unwrap();
        assert!(res.is_some());
        let v = res.unwrap();
        if let Some(n) = v.as_int() {
            assert_eq!(n, 42);
        } else {
            panic!("Expected Value::int(42)");
        }
    }

    #[test]
    fn test_jit_executor_cache() {
        let mut executor = JitExecutor::new().unwrap();
        let symbols = SymbolTable::new();
        let expr1 = Expr::Literal(Value::int(42));
        let expr2 = Expr::Literal(Value::int(43));

        let hash1 = CachedJitCode::compute_hash(&expr1);
        let hash2 = CachedJitCode::compute_hash(&expr2);

        executor.try_jit_execute(&expr1, &symbols).ok();
        executor.try_jit_execute(&expr2, &symbols).ok();

        let (compiled, total) = executor.cache_stats();
        assert!(total >= 2);
        assert!(compiled >= 2);
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_jit_executor_has_context() {
        let executor = JitExecutor::new().unwrap();
        // May or may not have context depending on system support
        let _ = executor.has_jit_context();
    }

    #[test]
    fn test_jit_compiled_code_creation() {
        let ptr = 0xdeadbeef as *const u8;
        let code = JitCompiledCode::new(ptr, 12345);
        assert_eq!(code.get_ptr(), ptr);
    }
}
