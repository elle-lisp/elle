//! MLIR compilation cache for the VM.
//!
//! Stores a shared MLIR Context and cached ExecutionEngines keyed by
//! bytecode pointer. The context is created once; subsequent compilations
//! amortize the 4ms initialization cost.

use crate::lir::LirFunction;
use melior::ExecutionEngine;
use std::collections::HashMap;

use super::lower::{create_context, lower_to_module, ScalarType};

/// Cached MLIR compilation state for the VM.
///
/// Owns a single MLIR Context (expensive to create) and a cache of
/// compiled ExecutionEngines keyed by bytecode pointer.
pub struct MlirCache {
    /// Shared MLIR context with all dialects registered.
    context: melior::Context,
    /// Compiled functions: (bytecode pointer, capture_types, param_types) → engine + function name + return type.
    engines: HashMap<(*const u8, u64, u64), (ExecutionEngine, String, ScalarType)>,
    /// Cached SPIR-V bytes: bytecode pointer → compiled SPIR-V binary.
    spirv_cache: HashMap<*const u8, Vec<u8>>,
    /// Functions that failed MLIR compilation — don't retry.
    rejections: std::collections::HashSet<(*const u8, u64, u64)>,
}

// Safety: MlirCache is only used from the single-threaded VM.
// The MLIR context and execution engines are not accessed concurrently.
unsafe impl Send for MlirCache {}
unsafe impl Sync for MlirCache {}

impl Default for MlirCache {
    fn default() -> Self {
        Self::new()
    }
}

impl MlirCache {
    pub fn new() -> Self {
        MlirCache {
            context: create_context(),
            engines: HashMap::new(),
            spirv_cache: HashMap::new(),
            rejections: std::collections::HashSet::new(),
        }
    }

    /// Record a compilation failure so we don't retry.
    pub fn reject(&mut self, key: *const u8, capture_types: u64, param_types: u64) {
        self.rejections.insert((key, capture_types, param_types));
    }

    /// Check if a function was previously rejected.
    pub fn is_rejected(&self, key: *const u8, capture_types: u64, param_types: u64) -> bool {
        self.rejections.contains(&(key, capture_types, param_types))
    }

    /// Compile a GPU-eligible LirFunction and cache the result.
    /// Returns the function name for subsequent invocation.
    pub fn compile(
        &mut self,
        key: *const u8,
        lir: &LirFunction,
        num_captures: u16,
        capture_types: u64,
        param_types: u64,
    ) -> Result<&str, String> {
        let (mut module, scalar_type) =
            lower_to_module(&self.context, lir, num_captures, capture_types, param_types)?;

        let pm = melior::pass::PassManager::new(&self.context);
        pm.add_pass(melior::pass::conversion::create_to_llvm());
        pm.run(&mut module)
            .map_err(|_| "MLIR-to-LLVM conversion failed".to_string())?;

        let engine = ExecutionEngine::new(&module, 2, &[], false, false);
        let name = lir.name.as_deref().unwrap_or("gpu_kernel").to_string();

        let cache_key = (key, capture_types, param_types);
        self.engines.insert(cache_key, (engine, name, scalar_type));
        Ok(&self.engines[&cache_key].1)
    }

    /// Get the return type for a cached function.
    pub fn return_type(
        &self,
        key: *const u8,
        capture_types: u64,
        param_types: u64,
    ) -> Option<ScalarType> {
        self.engines
            .get(&(key, capture_types, param_types))
            .map(|(_, _, st)| *st)
    }

    /// Call a cached MLIR-compiled function with i64 arguments.
    /// Returns the i64 result, or None if the function is not cached.
    pub fn call(
        &self,
        key: *const u8,
        args: &[i64],
        capture_types: u64,
        param_types: u64,
    ) -> Option<Result<i64, String>> {
        let (engine, name, _) = self.engines.get(&(key, capture_types, param_types))?;

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

    /// Check if a function is already compiled (CPU JIT).
    pub fn contains(&self, key: *const u8, capture_types: u64, param_types: u64) -> bool {
        self.engines
            .contains_key(&(key, capture_types, param_types))
    }

    /// Compile a GPU-eligible LirFunction to SPIR-V bytes, using the
    /// shared context and caching the result by bytecode pointer.
    pub fn compile_spirv(
        &mut self,
        key: *const u8,
        lir: &LirFunction,
        workgroup_size: u32,
    ) -> Result<&[u8], String> {
        if !self.spirv_cache.contains_key(&key) {
            let bytes =
                super::spirv::lower_to_spirv_with_context(&self.context, lir, workgroup_size)?;
            self.spirv_cache.insert(key, bytes);
        }
        Ok(&self.spirv_cache[&key])
    }

    /// Get cached SPIR-V bytes, if available.
    pub fn get_spirv(&self, key: *const u8) -> Option<&[u8]> {
        self.spirv_cache.get(&key).map(|v| v.as_slice())
    }
}
