// audited: 2026-11-25
//! Wasmtime Engine/Store/Linker setup, and the closure-env builder both
//! WASM hosts share.
//!
//! docs/impl/wasm.md

use wasmtime::*;

use super::host::ElleHost;
use crate::value::repr::TAG_HEAP_START;
use crate::value::Value;

mod call;
pub(in crate::wasm) use call::{call_precached_closure, call_wasm_closure, resume_wasm_closure};
pub use call::{compile_module, run_module};

/// Write the executing closure `(tag, payload)` into the reserved `SELF_SLOT`
/// (src/wasm/emit.rs) of `memory`. The host installs it at every closure entry so
/// a `LoadSelf` in the body reads the right closure. Generic over the store's host
/// type, so both the full-module (`ElleHost`) and tiered (`TieredHost`) paths, and
/// both `Caller` and `Store` contexts, share it.
pub(in crate::wasm) fn write_self_slot<T: 'static>(
    mut ctx: impl wasmtime::AsContextMut<Data = T>,
    memory: &wasmtime::Memory,
    tag: i64,
    payload: i64,
) {
    let base = super::emit::SELF_SLOT as usize;
    let data = memory.data_mut(ctx.as_context_mut());
    data[base..base + 8].copy_from_slice(&tag.to_le_bytes());
    data[base + 8..base + 16].copy_from_slice(&payload.to_le_bytes());
}

/// Take the signal a compiled function raised, clearing the channel behind it.
///
/// A compiled WASM function answers on two channels: the `status` word it
/// returns (0 = ran to completion, >0 = the resume state it suspended at), and
/// the `SignalBits` it raised, written to `SIGNAL_SLOT`. A caller that reads
/// only `status` reports a failed primitive as a successful return of the error
/// value — pinned by `wasm::tests::tier`.
///
/// Generic over the host type so the full-module (`ElleHost`) and tiered
/// (`TieredHost`) paths, and both `Caller` and `Store` contexts, share it.
pub(in crate::wasm) fn take_raised_signal<T: 'static>(
    mut ctx: impl wasmtime::AsContextMut<Data = T>,
    memory: &wasmtime::Memory,
) -> crate::value::fiber::SignalBits {
    let base = super::emit::SIGNAL_SLOT as usize;
    let data = memory.data_mut(ctx.as_context_mut());
    let raw = i64::from_le_bytes(data[base..base + 8].try_into().unwrap());
    if raw != 0 {
        data[base..base + 8].copy_from_slice(&0i64.to_le_bytes());
    }
    crate::value::fiber::SignalBits::new(raw as u64)
}

/// Read the executing closure `(tag, payload)` out of the reserved `SELF_SLOT`.
/// The mirror of [`write_self_slot`] — used to save/restore the slot around a
/// nested call and to snapshot it into a suspension frame at yield.
pub(in crate::wasm) fn read_self_slot<T: 'static>(
    ctx: impl wasmtime::AsContext<Data = T>,
    memory: &wasmtime::Memory,
) -> (i64, i64) {
    let base = super::emit::SELF_SLOT as usize;
    let data = memory.data(ctx.as_context());
    let tag = i64::from_le_bytes(data[base..base + 8].try_into().unwrap());
    let payload = i64::from_le_bytes(data[base + 8..base + 16].try_into().unwrap());
    (tag, payload)
}

/// Create a Wasmtime Engine with tail-call support, caching its compiles in
/// this build's `incremental` directory (docs/impl/wasm.md).
pub fn create_engine() -> Result<Engine> {
    let dir = super::cache::build_dir().map(|d| d.join("incremental"));
    create_engine_in(dir.as_deref())
}

/// Create a Wasmtime Engine with tail-call support, caching its compiles in
/// `cache` when one is named.
///
/// Cranelift optimizes the module at `OptLevel::Speed`. A `wasm` build runs no
/// JIT, so the JIT's policy has nothing to say about this tier.
pub fn create_engine_in(cache: Option<&std::path::Path>) -> Result<Engine> {
    let mut config = Config::new();
    config.wasm_tail_call(true);
    config.wasm_multi_value(true);
    config.cranelift_opt_level(OptLevel::Speed);
    if let Some(dir) = cache {
        super::cache::cache_compiles_in(&mut config, dir);
    }
    Engine::new(&config)
}

/// Create a Store with ElleHost state and pre-loaded constant pool.
pub fn create_store(
    engine: &Engine,
    const_pool: Vec<Value>,
    closure_bytecodes: super::host::ModuleCode,
    env_stack_base: usize,
) -> Store<ElleHost> {
    let mut host = ElleHost::new();
    // Start the env stack above this module's widest args region so no call's
    // args overwrite a live closure env (emit::env_stack_base).
    host.env_stack_ptr = env_stack_base;

    // Pre-load heap constants into handle table and build a mapping from
    // const pool index → handle index. Immediate values (symbols, keywords,
    // etc.) are NOT inserted into the handle table, so pool indices and
    // handle indices diverge when the pool contains a mix of types.
    let mut pool_to_handle = Vec::with_capacity(const_pool.len());
    for value in &const_pool {
        if value.tag >= TAG_HEAP_START {
            let handle = host.handles.insert(*value);
            pool_to_handle.push(handle);
        } else {
            pool_to_handle.push(0); // unused for immediates
        }
    }

    host.const_pool = const_pool;
    host.pool_to_handle = pool_to_handle;
    host.closure_bytecodes = closure_bytecodes;
    Store::new(engine, host)
}

/// Build a WASM closure's environment in linear memory at `env_base`.
///
/// Layout: `[captures...][params...][local_slots...]`, each slot 16 bytes.
/// Handles varargs, capture-cell wrapping, and memory growth.
/// Updates `env_stack_ptr` to point past the new env region.
///
/// Generic over host type: works with both `ElleHost` (full-module) and
/// `TieredHost` (per-closure) via the `WasmEnvHost` trait.
pub fn prepare_wasm_env<T: super::host::WasmEnvHost>(
    caller: &mut Caller<'_, T>,
    closure: &crate::value::closure::Closure,
    args: &[Value],
    env_base: usize,
) {
    let template = &closure.template;
    let num_captures = template.num_captures();
    let num_params = template.num_params();
    let num_locals = template.num_locals();
    let capture_locals_mask = &template.capture_locals_mask();

    // Each env value gets its OWN fresh per-execution region (mirroring the
    // interpreter's `env_value_region`, docs/impl/region/rules.md Rule 6, no
    // commingling). The heap is the driving instance's, reached through the host's
    // VM pointer (`WasmEnvHost::heap_ptr`); the raw pointer lets each
    // `value::build::*` call reborrow it for exactly one allocation (no two `&mut`
    // to the heap alive at once).
    let env_heap_ptr = caller.data().heap_ptr();
    let fresh_region = move || unsafe { (*env_heap_ptr).new_runtime_region() };

    // Handle varargs: if arity is AtLeast(n), collect extra args
    // into a list (or array) and pass as the last parameter.
    let effective_args;
    let args = match template.arity() {
        crate::value::types::Arity::AtLeast(required) => {
            let mut collected = Vec::with_capacity(num_params);
            for arg in args.iter().take(required) {
                collected.push(*arg);
            }
            let rest: Vec<Value> = args[required..].to_vec();
            let vararg_val = match template.vararg_kind() {
                crate::hir::VarargKind::List => {
                    // The whole vararg list shares one region.
                    let region = fresh_region();
                    let mut list = Value::EMPTY_LIST;
                    for v in rest.iter().rev() {
                        list = crate::value::build::pair(
                            unsafe { &mut *env_heap_ptr },
                            *v,
                            list,
                            region,
                        );
                    }
                    list
                }
                _ => {
                    let region = fresh_region();
                    crate::value::build::array_mut(unsafe { &mut *env_heap_ptr }, rest, region)
                }
            };
            collected.push(vararg_val);
            while collected.len() < num_params {
                collected.push(Value::NIL);
            }
            effective_args = collected;
            effective_args.as_slice()
        }
        _ => args,
    };

    let extra_locals = num_locals.saturating_sub(num_params);
    let total_slots = num_captures + num_params + extra_locals;
    caller
        .data_mut()
        .set_env_stack_ptr(env_base + total_slots * 16);

    let memory = caller
        .get_export("__elle_memory")
        .and_then(|e| e.into_memory())
        .expect("prepare_wasm_env: no memory");

    let needed_bytes = env_base + total_slots * 16;
    let current_bytes = memory.data_size(&*caller);
    if needed_bytes > current_bytes {
        let pages_needed = (needed_bytes - current_bytes).div_ceil(65536) as u64;
        memory
            .grow(&mut *caller, pages_needed)
            .expect("prepare_wasm_env: failed to grow memory");
    }

    // Helper: convert value to WASM repr and write to linear memory.
    let write_slot = |caller: &mut Caller<'_, T>, memory: &Memory, slot: usize, val: Value| {
        let (tag, payload) = caller.data_mut().value_to_wasm(val);
        let offset = env_base + slot * 16;
        let data = memory.data_mut(caller);
        data[offset..offset + 8].copy_from_slice(&tag.to_le_bytes());
        data[offset + 8..offset + 16].copy_from_slice(&payload.to_le_bytes());
    };

    // Captures
    for (i, val) in closure.env.iter().enumerate() {
        write_slot(&mut *caller, &memory, i, *val);
    }

    // Params, celled when the capture mask names them
    for (i, arg) in args.iter().enumerate().take(num_params) {
        let val = if template.param_is_celled(i) {
            let region = fresh_region();
            crate::value::build::capture_cell(
                unsafe { &mut *env_heap_ptr },
                *arg,
                crate::value::heap::CellOrigin::Runtime,
                region,
            )
        } else {
            *arg
        };
        write_slot(&mut *caller, &memory, num_captures + i, val);
    }

    // Remaining params (default nil)
    for i in args.len()..num_params {
        let val = if template.param_is_celled(i) {
            let region = fresh_region();
            crate::value::build::capture_cell(
                unsafe { &mut *env_heap_ptr },
                Value::NIL,
                crate::value::heap::CellOrigin::Runtime,
                region,
            )
        } else {
            Value::NIL
        };
        write_slot(&mut *caller, &memory, num_captures + i, val);
    }

    // Extra local slots: nil, or a capture cell holding nil. Precise at any
    // index: a captured local is celled, an uncaptured one (even >= 64) gets
    // bare NIL.
    for i in 0..extra_locals {
        let val = if capture_locals_mask.is_set(i) {
            let region = fresh_region();
            crate::value::build::capture_cell(
                unsafe { &mut *env_heap_ptr },
                Value::NIL,
                crate::value::heap::CellOrigin::Runtime,
                region,
            )
        } else {
            Value::NIL
        };
        write_slot(&mut *caller, &memory, num_captures + num_params + i, val);
    }
}
