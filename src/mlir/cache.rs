// audited: 2026-10-06
// docs/impl/mlir.md
//! MLIR compilation cache for the VM: one shared context, and the compiled
//! engines and the rejections, each pinning its key's code region.
//!
//! The context is created once; subsequent compilations amortize the 4ms
//! initialization cost. The SPIR-V compile borrows it, and the VM caches the
//! bytes (docs/impl/spirv.md).

use crate::lir::LirView;
use crate::value::CodePin;
use melior::ExecutionEngine;
use std::collections::HashMap;

use super::lower::{create_context, lower_to_module, ScalarType};

/// Which captures and which parameters arrive as floats: bit `i` of each mask
/// set means slot `i` is an `f64` bitcast to `i64`. One compiled function per
/// signature, so the pair is part of every cache key. Named fields, so a call
/// site cannot swap the two masks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct MlirSig {
    pub captures: u64,
    pub params: u64,
}

/// A table key: the bytecode address the pin names, and the signature.
type Key = (*const u8, MlirSig);

/// A compiled function, and the pin that keeps its key naming it.
struct Engine {
    engine: ExecutionEngine,
    name: String,
    returns: ScalarType,
    _pin: CodePin,
}

/// Cached MLIR compilation state for the VM.
///
/// Both tables key by bytecode address, so each entry holds a `CodePin` on
/// its key's code region and takes its key from that pin (docs/impl/jit.md).
pub struct MlirCache {
    /// Shared MLIR context with all dialects registered.
    context: melior::Context,
    /// Compiled functions, by bytecode address and signature.
    engines: HashMap<Key, Engine>,
    /// Functions that failed MLIR compilation, so no call retries them.
    rejections: HashMap<Key, CodePin>,
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
            rejections: HashMap::new(),
        }
    }

    /// Record a compilation failure so we don't retry. The entry holds `pin`,
    /// which names its key.
    pub fn reject(&mut self, pin: CodePin, sig: MlirSig) {
        self.rejections.entry((pin.key(), sig)).or_insert(pin);
    }

    /// Check if a function was previously rejected.
    pub fn is_rejected(&self, key: *const u8, sig: MlirSig) -> bool {
        self.rejections.contains_key(&(key, sig))
    }

    /// Drop every entry and the pin it holds, keeping the context.
    pub fn clear_pins(&mut self) {
        self.engines.clear();
        self.rejections.clear();
    }

    /// Compile a GPU-eligible frozen function and cache the result under the
    /// key `pin` names. Returns the function name for subsequent invocation.
    pub fn compile(
        &mut self,
        pin: CodePin,
        lir: &LirView<'_>,
        num_captures: u16,
        sig: MlirSig,
    ) -> Result<&str, String> {
        let (mut module, returns) =
            lower_to_module(&self.context, lir, num_captures, sig.captures, sig.params)?;

        let pm = melior::pass::PassManager::new(&self.context);
        pm.add_pass(melior::pass::conversion::create_to_llvm());
        pm.run(&mut module)
            .map_err(|_| "MLIR-to-LLVM conversion failed".to_string())?;

        let engine = ExecutionEngine::new(&module, 2, &[], false, false);
        let name = lir.name().unwrap_or("gpu_kernel").to_string();

        let key = (pin.key(), sig);
        self.engines.insert(
            key,
            Engine {
                engine,
                name,
                returns,
                _pin: pin,
            },
        );
        Ok(&self.engines[&key].name)
    }

    /// Get the return type for a cached function.
    pub fn return_type(&self, key: *const u8, sig: MlirSig) -> Option<ScalarType> {
        self.engines.get(&(key, sig)).map(|e| e.returns)
    }

    /// Call a cached MLIR-compiled function with i64 arguments.
    /// Returns the i64 result, or None if the function is not cached.
    pub fn call(&self, key: *const u8, args: &[i64], sig: MlirSig) -> Option<Result<i64, String>> {
        let compiled = self.engines.get(&(key, sig))?;

        let mut arg_values: Vec<i64> = args.to_vec();
        let mut result: i64 = 0;

        let mut packed: Vec<*mut ()> = Vec::new();
        for arg in &mut arg_values {
            packed.push(arg as *mut i64 as *mut ());
        }
        packed.push(&mut result as *mut i64 as *mut ());

        let invoke_result = unsafe { compiled.engine.invoke_packed(&compiled.name, &mut packed) };

        Some(match invoke_result {
            Ok(()) => Ok(result),
            Err(e) => Err(format!("MLIR execution failed: {:?}", e)),
        })
    }

    /// Check if a function is already compiled (CPU JIT).
    pub fn contains(&self, key: *const u8, sig: MlirSig) -> bool {
        self.engines.contains_key(&(key, sig))
    }

    /// Compile a GPU-eligible frozen function to SPIR-V bytes at
    /// `workgroup_size`, through the shared context. Caches nothing: the VM's
    /// SPIR-V cache holds the bytes, pinned (docs/impl/spirv.md).
    pub fn compile_spirv(&self, lir: &LirView<'_>, workgroup_size: u32) -> Result<Vec<u8>, String> {
        super::spirv::lower_to_spirv_with_context(&self.context, lir, workgroup_size)
    }
}
