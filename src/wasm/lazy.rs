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
        if let Ok(cache_dir) = std::env::var("ELLE_WASM_CACHE") {
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
    pub fn compile(&mut self, bytecode_ptr: *const u8, lir_func: &LirFunction) -> bool {
        if self.modules.contains_key(&bytecode_ptr) {
            return true;
        }

        let result = match emit::emit_single_closure(lir_func) {
            Some(r) => r,
            None => return false, // Can't compile this closure standalone
        };

        match Module::new(&self.engine, &result.wasm_bytes) {
            Ok(module) => {
                if std::env::var_os("ELLE_WASM_DEBUG").is_some() {
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
                if std::env::var_os("ELLE_WASM_DEBUG").is_some() {
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

        // Call the closure function
        let func = instance
            .get_typed_func::<(i32, i32, i32, i32), (i64, i64, i32)>(&mut store, "__elle_closure")
            .map_err(|e| e.to_string())?;

        let (tag, payload, status) = func
            .call(&mut store, (env_base as i32, 0, 0, 0))
            .map_err(|e| e.to_string())?;

        let value = store.data().inner.wasm_to_value(tag, payload);
        let signal = SignalBits::new(status as u32);
        Ok((value, signal))
    }
}

/// Build closure environment in WASM linear memory.
///
/// Layout: [captures...] [params...] [local_slots(zeroed)...]
/// Each slot is 16 bytes (tag: i64, payload: i64).
/// Follows the same pattern as `prepare_wasm_env` in store.rs:
/// interleaves `value_to_wasm` with `data_mut` to avoid borrow issues.
fn build_env_in_memory(
    store: &mut Store<TieredHost>,
    memory: &Memory,
    closure: &crate::value::closure::Closure,
    args: &[Value],
    env_base: usize,
) {
    let template = &closure.template;
    let num_captures = template.num_captures;
    let num_params = template.num_params;
    let num_locals = template.num_locals;
    let lbox_params_mask = template.lbox_params_mask;
    let lbox_locals_mask = template.lbox_locals_mask;
    let extra_locals = num_locals.saturating_sub(num_params);
    let total_slots = num_captures + num_params + extra_locals;

    // Ensure memory is large enough
    let needed_bytes = env_base + total_slots * 16;
    let current_bytes = memory.data_size(&*store);
    if needed_bytes > current_bytes {
        let pages_needed = (needed_bytes - current_bytes).div_ceil(65536) as u64;
        memory.grow(&mut *store, pages_needed).ok();
    }

    // Write captures
    for (i, val) in closure.env.iter().enumerate() {
        let (tag, payload) = store.data_mut().inner.value_to_wasm(*val);
        let offset = env_base + i * 16;
        let data = memory.data_mut(&mut *store);
        data[offset..offset + 8].copy_from_slice(&tag.to_le_bytes());
        data[offset + 8..offset + 16].copy_from_slice(&payload.to_le_bytes());
    }

    // Write params with optional LBox wrapping
    for (i, arg) in args.iter().enumerate().take(num_params) {
        let val = if i < 64 && lbox_params_mask & (1u64 << i) != 0 {
            Value::local_lbox(*arg)
        } else {
            *arg
        };
        let (tag, payload) = store.data_mut().inner.value_to_wasm(val);
        let offset = env_base + (num_captures + i) * 16;
        let data = memory.data_mut(&mut *store);
        data[offset..offset + 8].copy_from_slice(&tag.to_le_bytes());
        data[offset + 8..offset + 16].copy_from_slice(&payload.to_le_bytes());
    }

    // Write nil for remaining params
    for i in args.len()..num_params {
        let val = if i < 64 && lbox_params_mask & (1u64 << i) != 0 {
            Value::local_lbox(Value::NIL)
        } else {
            Value::NIL
        };
        let (tag, payload) = store.data_mut().inner.value_to_wasm(val);
        let offset = env_base + (num_captures + i) * 16;
        let data = memory.data_mut(&mut *store);
        data[offset..offset + 8].copy_from_slice(&tag.to_le_bytes());
        data[offset + 8..offset + 16].copy_from_slice(&payload.to_le_bytes());
    }

    // Write nil/LBox(nil) for extra local slots
    for i in 0..extra_locals {
        let val = if i < 64 && lbox_locals_mask & (1u64 << i) != 0 {
            Value::local_lbox(Value::NIL)
        } else {
            Value::NIL
        };
        let (tag, payload) = store.data_mut().inner.value_to_wasm(val);
        let offset = env_base + (num_captures + num_params + i) * 16;
        let data = memory.data_mut(&mut *store);
        data[offset..offset + 8].copy_from_slice(&tag.to_le_bytes());
        data[offset + 8..offset + 16].copy_from_slice(&payload.to_le_bytes());
    }
}

/// Create a Linker with host functions for tiered execution.
///
/// Differs from the full WASM linker in that rt_call and
/// rt_prepare_tail_call can handle bytecode closures by calling back
/// into the VM.
fn create_tiered_linker(engine: &Engine) -> Result<Linker<TieredHost>> {
    let mut linker: Linker<TieredHost> = Linker::new(engine);

    // call_primitive — same as full backend
    linker.func_wrap(
        "elle",
        "call_primitive",
        |mut caller: Caller<'_, TieredHost>,
         prim_id: i32,
         args_ptr: i32,
         nargs: i32,
         _ctx: i32|
         -> (i64, i64, i32) {
            let args = read_args(&mut caller, args_ptr, nargs);
            let (bits, result) = caller
                .data_mut()
                .inner
                .call_primitive(prim_id as u32, &args);
            let (bits, result) = caller.data().inner.maybe_execute_io(bits, result);
            let (tag, payload) = caller.data_mut().inner.value_to_wasm(result);
            (tag, payload, bits.raw() as i32)
        },
    )?;

    // rt_call — handles both WASM and bytecode closures
    linker.func_wrap(
        "elle",
        "rt_call",
        |mut caller: Caller<'_, TieredHost>,
         func_tag: i64,
         func_payload: i64,
         args_ptr: i32,
         nargs: i32,
         _ctx: i32|
         -> (i64, i64, i32) {
            let func_val = caller.data().inner.wasm_to_value(func_tag, func_payload);
            let args = read_args(&mut caller, args_ptr, nargs);

            if func_val.is_native_fn() {
                let native_fn = func_val.as_native_fn().unwrap();
                let (bits, result) = native_fn(&args);
                let (bits, result) = caller.data().inner.maybe_execute_io(bits, result);
                let (tag, payload) = caller.data_mut().inner.value_to_wasm(result);
                return (tag, payload, bits.raw() as i32);
            }

            if let Some((id, default)) = func_val.as_parameter() {
                if !args.is_empty() {
                    let err = crate::value::error_val(
                        "arity-error",
                        format!("parameter call: expected 0 arguments, got {}", args.len()),
                    );
                    let (tag, payload) = caller.data_mut().inner.value_to_wasm(err);
                    return (tag, payload, 1);
                }
                let value = caller.data().inner.resolve_parameter(id, default);
                let (tag, payload) = caller.data_mut().inner.value_to_wasm(value);
                return (tag, payload, 0);
            }

            if let Some(closure) = func_val.as_closure() {
                let bytecode_ptr = closure.template.bytecode.as_ptr();
                let current_ptr = caller.data().current_bytecode_ptr;

                // Self-recursive call: dispatch directly through the instance table.
                // The current function is at table index 0.
                if bytecode_ptr == current_ptr {
                    let env_base = caller.data().inner.env_stack_ptr;
                    super::store::prepare_wasm_env(&mut caller, closure, &args, env_base);

                    let table = caller
                        .get_export("__elle_table")
                        .and_then(|e| e.into_table())
                        .expect("rt_call: no table");
                    let func_ref = table
                        .get(&mut caller, 0)
                        .expect("rt_call: table index 0 missing");
                    let func = func_ref
                        .unwrap_func()
                        .expect("rt_call: table entry is not a function");

                    let mut results = [Val::I64(0), Val::I64(0), Val::I32(0)];
                    match func.call(
                        &mut caller,
                        &[
                            Val::I32(env_base as i32),
                            Val::I32(0),
                            Val::I32(0),
                            Val::I32(0),
                        ],
                        &mut results,
                    ) {
                        Ok(()) => {
                            let tag = results[0].unwrap_i64();
                            let payload = results[1].unwrap_i64();
                            let status = results[2].unwrap_i32();
                            // Restore env_stack_ptr after the call
                            caller.data_mut().inner.env_stack_ptr = env_base;
                            return (tag, payload, status);
                        }
                        Err(e) => {
                            let err = crate::value::error_val(
                                "internal-error",
                                format!("wasm self-call: {}", e),
                            );
                            let (tag, payload) = caller.data_mut().inner.value_to_wasm(err);
                            return (tag, payload, 1);
                        }
                    }
                }

                let vm = caller.data().vm;
                let vm_ref = unsafe { &mut *vm };

                // Check if this closure has a WASM-compiled version.
                // If so, call it directly (re-entrant WASM call via new Store).
                let has_wasm = vm_ref
                    .wasm_tier
                    .as_ref()
                    .map(|t| t.is_compiled(bytecode_ptr))
                    .unwrap_or(false);

                if has_wasm {
                    let closure_rc = std::rc::Rc::new((*closure).clone());
                    let wasm_tier = vm_ref.wasm_tier.as_ref().unwrap();
                    match wasm_tier.call(vm, bytecode_ptr, &closure_rc, &args) {
                        Ok((value, signal)) => {
                            if signal.is_ok() || signal == crate::value::SIG_HALT {
                                let (tag, payload) = caller.data_mut().inner.value_to_wasm(value);
                                return (tag, payload, 0);
                            }
                            let (tag, payload) = caller.data_mut().inner.value_to_wasm(value);
                            return (tag, payload, signal.raw() as i32);
                        }
                        Err(e) => {
                            let err =
                                crate::value::error_val("internal-error", format!("wasm: {}", e));
                            let (tag, payload) = caller.data_mut().inner.value_to_wasm(err);
                            return (tag, payload, 1);
                        }
                    }
                }

                // Bytecode closure: call back into the VM
                match vm_ref.build_closure_env(closure, &args) {
                    Some(env) => {
                        let exec = vm_ref.execute_bytecode_saving_stack(
                            &closure.template.bytecode,
                            &closure.template.constants,
                            &env,
                            &closure.template.location_map,
                        );
                        let bits = exec.bits;
                        if bits.is_ok() || bits == crate::value::SIG_HALT {
                            let (_, val) = vm_ref.fiber.signal.take().unwrap();
                            let (tag, payload) = caller.data_mut().inner.value_to_wasm(val);
                            (tag, payload, 0)
                        } else {
                            let val = vm_ref
                                .fiber
                                .signal
                                .as_ref()
                                .map(|(_, v)| *v)
                                .unwrap_or(Value::NIL);
                            let (tag, payload) = caller.data_mut().inner.value_to_wasm(val);
                            (tag, payload, bits.raw() as i32)
                        }
                    }
                    None => {
                        let val = vm_ref
                            .fiber
                            .signal
                            .as_ref()
                            .map(|(_, v)| *v)
                            .unwrap_or(Value::NIL);
                        let bits = vm_ref
                            .fiber
                            .signal
                            .as_ref()
                            .map(|(b, _)| *b)
                            .unwrap_or(crate::value::SIG_ERROR);
                        let (tag, payload) = caller.data_mut().inner.value_to_wasm(val);
                        (tag, payload, bits.raw() as i32)
                    }
                }
            } else {
                let err = crate::value::error_val(
                    "type-error",
                    format!("rt_call: cannot call {}", func_val.type_name()),
                );
                let (tag, payload) = caller.data_mut().inner.value_to_wasm(err);
                (tag, payload, 1)
            }
        },
    )?;

    // rt_load_const — same as full backend
    linker.func_wrap(
        "elle",
        "rt_load_const",
        |caller: Caller<'_, TieredHost>, index: i32| -> (i64, i64) {
            let host = &caller.data().inner;
            let value = host.const_pool[index as usize];
            if value.tag < TAG_HEAP_START {
                (value.tag as i64, value.payload as i64)
            } else {
                let handle = host.pool_to_handle[index as usize];
                (value.tag as i64, handle as i64)
            }
        },
    )?;

    // rt_data_op — same as full backend
    linker.func_wrap(
        "elle",
        "rt_data_op",
        |mut caller: Caller<'_, TieredHost>,
         op: i32,
         args_ptr: i32,
         nargs: i32|
         -> (i64, i64, i32) {
            let args = read_args(&mut caller, args_ptr, nargs);
            let (bits, result) = super::linker::dispatch_data_op(op, &args);
            let (tag, payload) = caller.data_mut().inner.value_to_wasm(result);
            (tag, payload, bits.raw() as i32)
        },
    )?;

    // rt_make_closure — stub (we reject MakeClosure at emit time)
    linker.func_wrap(
        "elle",
        "rt_make_closure",
        |_caller: Caller<'_, TieredHost>,
         _table_idx: i32,
         _captures_ptr: i32,
         _metadata_ptr: i32|
         -> (i64, i64) {
            panic!("rt_make_closure called in tiered mode — should not happen");
        },
    )?;

    // rt_push_param
    linker.func_wrap(
        "elle",
        "rt_push_param",
        |mut caller: Caller<'_, TieredHost>, args_ptr: i32, npairs: i32| -> () {
            let mut pairs = Vec::new();
            let memory = caller
                .get_export("__elle_memory")
                .and_then(|e| e.into_memory())
                .expect("no memory");
            let data = memory.data(&caller);
            for i in 0..npairs as usize {
                let offset = args_ptr as usize + i * 24;
                let param_id =
                    i64::from_le_bytes(data[offset..offset + 8].try_into().unwrap()) as u32;
                let tag =
                    i64::from_le_bytes(data[offset + 8..offset + 16].try_into().unwrap()) as u64;
                let payload =
                    i64::from_le_bytes(data[offset + 16..offset + 24].try_into().unwrap()) as u64;
                let value = if tag < TAG_HEAP_START {
                    Value { tag, payload }
                } else {
                    caller.data().inner.handles.get(payload)
                };
                pairs.push((param_id, value));
            }
            caller.data_mut().inner.param_frames.push(pairs);
        },
    )?;

    // rt_pop_param
    linker.func_wrap(
        "elle",
        "rt_pop_param",
        |mut caller: Caller<'_, TieredHost>| -> () {
            caller.data_mut().inner.param_frames.pop();
        },
    )?;

    // rt_prepare_tail_call — stub (we reject TailCall at emit time)
    linker.func_wrap(
        "elle",
        "rt_prepare_tail_call",
        |_caller: Caller<'_, TieredHost>,
         _func_tag: i64,
         _func_payload: i64,
         _args_ptr: i32,
         _nargs: i32,
         _caller_env_ptr: i32|
         -> (i32, i32, i32, i64, i64, i32) {
            panic!("rt_prepare_tail_call called in tiered mode — should not happen");
        },
    )?;

    // rt_yield — stub (we reject Yield at emit time)
    linker.func_wrap(
        "elle",
        "rt_yield",
        |_caller: Caller<'_, TieredHost>,
         _tag: i64,
         _payload: i64,
         _resume_state: i32,
         _regs_ptr: i32,
         _num_regs: i32,
         _func_idx: i32|
         -> () {
            panic!("rt_yield called in tiered mode — should not happen");
        },
    )?;

    // rt_get_resume_value — stub
    linker.func_wrap(
        "elle",
        "rt_get_resume_value",
        |_caller: Caller<'_, TieredHost>| -> (i64, i64) {
            panic!("rt_get_resume_value called in tiered mode — should not happen");
        },
    )?;

    // rt_load_saved_reg — stub
    linker.func_wrap(
        "elle",
        "rt_load_saved_reg",
        |_caller: Caller<'_, TieredHost>, _index: i32| -> (i64, i64) {
            panic!("rt_load_saved_reg called in tiered mode — should not happen");
        },
    )?;

    Ok(linker)
}

/// Read args from WASM linear memory (same as full backend).
fn read_args(caller: &mut Caller<'_, TieredHost>, args_ptr: i32, nargs: i32) -> Vec<Value> {
    let memory = caller
        .get_export("__elle_memory")
        .and_then(|e| e.into_memory())
        .expect("read_args: no memory");
    let data = memory.data(&*caller);
    let mut args = Vec::with_capacity(nargs.max(0) as usize);
    for i in 0..nargs.max(0) as usize {
        let offset = args_ptr as usize + i * 16;
        let tag = i64::from_le_bytes(data[offset..offset + 8].try_into().unwrap()) as u64;
        let payload = i64::from_le_bytes(data[offset + 8..offset + 16].try_into().unwrap()) as u64;
        let value = if tag < TAG_HEAP_START {
            Value { tag, payload }
        } else {
            caller.data().inner.handles.get(payload)
        };
        args.push(value);
    }
    args
}
