//! MLIR compilation cache for the VM.
//!
//! Stores a shared MLIR Context and cached ExecutionEngines keyed by
//! bytecode pointer. The context is created once; subsequent compilations
//! amortize the 4ms initialization cost.

use crate::lir::LirFunction;
use melior::ExecutionEngine;
use std::collections::HashMap;

use super::lower::{create_context, lower_to_module};

/// Cached MLIR compilation state for the VM.
///
/// Owns a single MLIR Context (expensive to create) and a cache of
/// compiled ExecutionEngines keyed by bytecode pointer.
pub struct MlirCache {
    /// Shared MLIR context with all dialects registered.
    context: melior::Context,
    /// Compiled functions: bytecode pointer → engine + function name.
    engines: HashMap<*const u8, (ExecutionEngine, String)>,
}

// Safety: MlirCache is only used from the single-threaded VM.
// The MLIR context and execution engines are not accessed concurrently.
unsafe impl Send for MlirCache {}
unsafe impl Sync for MlirCache {}

impl MlirCache {
    pub fn new() -> Self {
        MlirCache {
            context: create_context(),
            engines: HashMap::new(),
        }
    }

    /// Compile a GPU-eligible LirFunction and cache the result.
    /// Returns the function name for subsequent invocation.
    pub fn compile(&mut self, key: *const u8, lir: &LirFunction) -> Result<&str, String> {
        let mut module = lower_to_module(&self.context, lir)?;

        let pm = melior::pass::PassManager::new(&self.context);
        pm.add_pass(melior::pass::conversion::create_to_llvm());
        pm.run(&mut module)
            .map_err(|_| "MLIR-to-LLVM conversion failed".to_string())?;

        let engine = ExecutionEngine::new(&module, 2, &[], false, false);
        let name = lir.name.as_deref().unwrap_or("gpu_kernel").to_string();

        self.engines.insert(key, (engine, name));
        Ok(&self.engines[&key].1)
    }

    /// Call a cached MLIR-compiled function with i64 arguments.
    /// Returns the i64 result, or None if the function is not cached.
    pub fn call(&self, key: *const u8, args: &[i64]) -> Option<Result<i64, String>> {
        let (engine, name) = self.engines.get(&key)?;

        let mut arg_values: Vec<i64> = args.to_vec();
        let mut result: i64 = 0;

        let mut packed: Vec<*mut ()> = Vec::new();
        for arg in &mut arg_values {
            packed.push(arg as *mut i64 as *mut ());
        }
        packed.push(&mut result as *mut i64 as *mut ());

        let invoke_result = unsafe { engine.invoke_packed(name, &mut packed) };

        Some(match invoke_result {
            Ok(()) => Ok(result),
            Err(e) => Err(format!("MLIR execution failed: {:?}", e)),
        })
    }

    /// Check if a function is already compiled.
    pub fn contains(&self, key: *const u8) -> bool {
        self.engines.contains_key(&key)
    }
}
