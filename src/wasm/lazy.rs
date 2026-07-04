//! Lazy (tiered) WASM compilation.
//!
//! Compiles individual hot closures to WASM on demand. The bytecode VM
//! remains the primary execution engine; closures that exceed a call-count
//! threshold get compiled to single-function WASM modules and dispatched
//! through Wasmtime.
//!
//! Architecture:
//!   VM call path → try_wasm_call → WasmTier::call
//!   WASM closure → rt_call → host → VM (for bytecode closures)

use crate::lir::LirFunction;
use crate::value::repr::TAG_HEAP_START;
use crate::value::{SignalBits, Value};
use rustc_hash::FxHashMap;
use std::rc::Rc;
use wasmtime::*;

use super::emit;
use super::host::ElleHost;

mod env;
use env::*;

/// Compiled single-closure WASM module ready for instantiation.
struct CompiledClosure {
    module: Module,
    const_pool: Vec<Value>,
}

/// Manages lazy WASM compilation for the tiered execution model.
///
/// Holds a Wasmtime `Engine` and `Linker` shared across all compiled
/// closures. Each hot closure gets its own `Module` cached by bytecode
/// pointer.
pub struct WasmTier {
    engine: Engine,
    linker: Linker<TieredHost>,
    /// Cache: bytecode pointer → compiled WASM module.
    modules: FxHashMap<*const u8, CompiledClosure>,
}

/// Host state for tiered execution. Extends ElleHost with a VM pointer
/// for calling back into the bytecode interpreter.
pub struct TieredHost {
    pub inner: ElleHost,
    /// Raw pointer to the VM. Valid for the duration of a WASM call.
    /// Used by rt_call to dispatch bytecode closures back to the VM.
    pub vm: *mut crate::vm::VM,
    /// Bytecode pointer of the currently executing WASM function.
    /// Used by rt_call to detect self-recursive calls and dispatch
    /// them directly through the instance table instead of creating
    /// a new Store.
    pub current_bytecode_ptr: *const u8,
}

impl super::host::WasmEnvHost for TieredHost {
    fn env_stack_ptr(&self) -> usize {
        self.inner.env_stack_ptr
    }
    fn set_env_stack_ptr(&mut self, ptr: usize) {
        self.inner.env_stack_ptr = ptr;
    }
    fn value_to_wasm(&mut self, value: crate::value::Value) -> (i64, i64) {
        self.inner.value_to_wasm(value)
    }
    fn heap_ptr(&self) -> *mut crate::value::fiberheap::FiberHeap {
        unsafe { (*self.vm).heap_ptr }
    }
}

impl WasmTier {
    /// Create a new WasmTier with engine and linker.
    pub fn new() -> Result<Self, String> {
        let mut config = Config::new();
        config.wasm_tail_call(true);
        config.wasm_multi_value(true);
        // Fast compile for per-closure modules (they're tiny).
        config.cranelift_opt_level(OptLevel::Speed);

        // Disk cache for incremental compilation
        if let Some(cache_dir) = &crate::config::get().cache {
            let path = std::path::PathBuf::from(&cache_dir).join("tiered");
            std::fs::create_dir_all(&path).ok();
            let cache = super::store::DiskCache::new(path);
            config
                .enable_incremental_compilation(std::sync::Arc::new(cache))
                .ok();
        }

        let engine = Engine::new(&config).map_err(|e| e.to_string())?;
        let linker = create_tiered_linker(&engine).map_err(|e| e.to_string())?;

        Ok(WasmTier {
            engine,
            linker,
            modules: FxHashMap::default(),
        })
    }

    /// Try to compile a closure to WASM. Returns true if compilation succeeded.
    /// `heap_ptr` is the driving instance's heap, on which the closure's
    /// const-pool literals are built (held for the cached module's lifetime).
    pub fn compile(
        &mut self,
        bytecode_ptr: *const u8,
        lir_func: &LirFunction,
        heap_ptr: *mut crate::value::fiberheap::FiberHeap,
    ) -> bool {
        if self.modules.contains_key(&bytecode_ptr) {
            return true;
        }

        let result = match emit::emit_single_closure(lir_func, None, heap_ptr) {
            Some(r) => r,
            None => return false, // Can't compile this closure standalone
        };

        match Module::new(&self.engine, &result.wasm_bytes) {
            Ok(module) => {
                if crate::config::get().has_trace("wasm") {
                    eprintln!(
                        "[wasm-tier] compiled {:?} ({} bytes, {} consts)",
                        lir_func.name,
                        result.wasm_bytes.len(),
                        result.const_pool.len()
                    );
                }
                self.modules.insert(
                    bytecode_ptr,
                    CompiledClosure {
                        module,
                        const_pool: result.const_pool,
                    },
                );
                true
            }
            Err(e) => {
                if crate::config::get().has_trace("wasm") {
                    eprintln!("[wasm-tier] compile failed for {:?}: {}", lir_func.name, e);
                }
                false
            }
        }
    }

    /// Check if a closure has been WASM-compiled.
    pub fn is_compiled(&self, bytecode_ptr: *const u8) -> bool {
        self.modules.contains_key(&bytecode_ptr)
    }

    /// Call a WASM-compiled closure.
    ///
    /// # Safety
    /// `vm` must be valid for the duration of the call. The caller must
    /// ensure no other mutable references to the VM exist during execution
    /// (same safety contract as the JIT path).
    pub fn call(
        &self,
        vm: *mut crate::vm::VM,
        bytecode_ptr: *const u8,
        closure: &Rc<crate::value::closure::Closure>,
        args: &[Value],
        self_val: Value,
    ) -> Result<(Value, SignalBits), String> {
        let compiled = self
            .modules
            .get(&bytecode_ptr)
            .expect("wasm_tier::call: closure not compiled");

        // Create a fresh Store with the const pool and VM pointer.
        let mut host = ElleHost::new();
        let mut pool_to_handle = Vec::with_capacity(compiled.const_pool.len());
        for value in &compiled.const_pool {
            if value.tag >= TAG_HEAP_START {
                let handle = host.handles.insert(*value);
                pool_to_handle.push(handle);
            } else {
                pool_to_handle.push(0);
            }
        }
        host.const_pool = compiled.const_pool.clone();
        host.pool_to_handle = pool_to_handle;
        // The inner host dispatches primitives, so it carries the driving VM too
        // (the lazy `rt_call`/`rt_data_op` reach it via `caller.data().vm`).
        host.vm = vm;

        let tiered_host = TieredHost {
            inner: host,
            vm,
            current_bytecode_ptr: bytecode_ptr,
        };
        let mut store = Store::new(&self.engine, tiered_host);

        let instance = self
            .linker
            .instantiate(&mut store, &compiled.module)
            .map_err(|e| e.to_string())?;

        // Build env in linear memory (captures + params + local slots)
        let memory = instance
            .get_memory(&mut store, "__elle_memory")
            .expect("no memory");
        let env_base = super::host::ENV_STACK_BASE;
        build_env_in_memory(&mut store, &memory, closure, args, env_base);

        // Install the executing closure in this fresh store's self slot (its own
        // handle space) so a `LoadSelf` in the body resolves to it.
        let (self_tag, self_payload) = store.data_mut().inner.value_to_wasm(self_val);
        super::store::write_self_slot(&mut store, &memory, self_tag, self_payload);

        // Call the closure function
        let func = instance
            .get_typed_func::<(i32, i32, i32, i32), (i64, i64, i64)>(&mut store, "__elle_closure")
            .map_err(|e| e.to_string())?;

        let (tag, payload, status) = func
            .call(&mut store, (env_base as i32, 0, 0, 0))
            .map_err(|e| e.to_string())?;

        let value = store.data().inner.wasm_to_value(tag, payload);
        let signal = SignalBits::new(status as u64);
        Ok((value, signal))
    }
}
