// audited: 2026-09-19
//! Calling a precached per-closure module from a full-module `rt_call`,
//! through a fresh standalone store.
//!
//! docs/impl/wasm.md

use wasmtime::{Caller, Memory, Store};

use crate::wasm::host::ElleHost;
use crate::wasm::outcome::CallOutcome;
use crate::wasm::store::{take_raised_signal, write_self_slot};

/// Build closure env in linear memory for a standalone `Store<ElleHost>`.
///
/// Same layout as `prepare_wasm_env`: \[captures\]\[params\]\[locals\], each 16 bytes.
fn build_env_in_store(
    store: &mut Store<ElleHost>,
    memory: &Memory,
    closure: &crate::value::closure::Closure,
    args: &[crate::value::Value],
    env_base: usize,
) {
    let template = &closure.template;
    let num_captures = template.num_captures();
    let num_params = template.num_params();
    let num_locals = template.num_locals();
    let capture_params_mask = template.capture_params_mask();
    let capture_locals_mask = &template.capture_locals_mask();
    let extra_locals = num_locals.saturating_sub(num_params);
    let total_slots = num_captures + num_params + extra_locals;

    // Each capture cell gets its OWN fresh per-execution region (mirroring the
    // interpreter's `env_value_region`, Rule 6). The heap is the wasm host's,
    // reached through the raw pointer so each `value::build::*` call reborrows it
    // for exactly one allocation (no two `&mut` to the heap alive at once).
    let env_heap_ptr = store.data().heap_ptr();
    let fresh_region = move || unsafe { (*env_heap_ptr).new_runtime_region() };

    let needed_bytes = env_base + total_slots * 16;
    let current_bytes = memory.data_size(&*store);
    if needed_bytes > current_bytes {
        let pages_needed = (needed_bytes - current_bytes).div_ceil(65536) as u64;
        memory.grow(&mut *store, pages_needed).ok();
    }

    // Write captures
    for (i, val) in closure.env.iter().enumerate() {
        let (tag, payload) = store.data_mut().value_to_wasm(*val);
        let offset = env_base + i * 16;
        let data = memory.data_mut(&mut *store);
        data[offset..offset + 8].copy_from_slice(&tag.to_le_bytes());
        data[offset + 8..offset + 16].copy_from_slice(&payload.to_le_bytes());
    }

    // Write params, celled when the capture mask names them
    for (i, arg) in args.iter().enumerate().take(num_params) {
        let val = if i < 64 && capture_params_mask & (1u64 << i) != 0 {
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
        let (tag, payload) = store.data_mut().value_to_wasm(val);
        let offset = env_base + (num_captures + i) * 16;
        let data = memory.data_mut(&mut *store);
        data[offset..offset + 8].copy_from_slice(&tag.to_le_bytes());
        data[offset + 8..offset + 16].copy_from_slice(&payload.to_le_bytes());
    }

    // Remaining params default to nil
    for i in args.len()..num_params {
        let val = if i < 64 && capture_params_mask & (1u64 << i) != 0 {
            let region = fresh_region();
            crate::value::build::capture_cell(
                unsafe { &mut *env_heap_ptr },
                crate::value::Value::NIL,
                crate::value::heap::CellOrigin::Runtime,
                region,
            )
        } else {
            crate::value::Value::NIL
        };
        let (tag, payload) = store.data_mut().value_to_wasm(val);
        let offset = env_base + (num_captures + i) * 16;
        let data = memory.data_mut(&mut *store);
        data[offset..offset + 8].copy_from_slice(&tag.to_le_bytes());
        data[offset + 8..offset + 16].copy_from_slice(&payload.to_le_bytes());
    }

    // Extra local slots
    for i in 0..extra_locals {
        // Precise at any index: a captured local is celled, an uncaptured one
        // (even >= 64) gets bare NIL.
        let val = if capture_locals_mask.is_set(i) {
            let region = fresh_region();
            crate::value::build::capture_cell(
                unsafe { &mut *env_heap_ptr },
                crate::value::Value::NIL,
                crate::value::heap::CellOrigin::Runtime,
                region,
            )
        } else {
            crate::value::Value::NIL
        };
        let (tag, payload) = store.data_mut().value_to_wasm(val);
        let offset = env_base + (num_captures + num_params + i) * 16;
        let data = memory.data_mut(&mut *store);
        data[offset..offset + 8].copy_from_slice(&tag.to_le_bytes());
        data[offset + 8..offset + 16].copy_from_slice(&payload.to_le_bytes());
    }
}

/// Call a pre-compiled per-closure Module from within a full-module rt_call.
///
/// Creates a fresh Store for the standalone Module, builds the closure's
/// env in the new Store's linear memory, calls the function, and converts
/// the result back to the caller's handle space.
pub(in crate::wasm) fn call_precached_closure(
    caller: &mut Caller<'_, ElleHost>,
    closure: &crate::value::closure::Closure,
    pc: &crate::wasm::host::PrecachedClosure,
    args: &[crate::value::Value],
    self_val: crate::value::Value,
) -> CallOutcome {
    use crate::value::repr::TAG_HEAP_START;

    let engine = caller.engine().clone();
    let mut host = ElleHost::new();

    // Use the standalone module's OWN const pool — its rt_load_const
    // indices are relative to this pool, not the full module's.
    host.const_pool = pc.const_pool.clone();
    let mut pool_to_handle = Vec::with_capacity(host.const_pool.len());
    for value in &host.const_pool {
        if value.tag >= TAG_HEAP_START {
            let handle = host.handles.insert(*value);
            pool_to_handle.push(handle);
        } else {
            pool_to_handle.push(0);
        }
    }
    host.pool_to_handle = pool_to_handle;
    // Copy precached_closures so nested calls can dispatch too.
    host.precached_closures = caller.data().precached_closures.clone();
    // Inherit the enclosing call's driving VM so this nested host's primitives
    // build a VM-bearing `NativeCtx`.
    host.vm = caller.data().vm;

    let mut store = Store::new(&engine, host);
    let linker = match crate::wasm::linker::create_linker(&engine) {
        Ok(l) => l,
        Err(e) => {
            let heap = unsafe { &mut *caller.data().heap_ptr() };
            let ctx = crate::primitives::ctx::Alloc::new(heap);
            let err = ctx.error("internal-error", e.to_string());
            let (tag, payload) = caller.data_mut().value_to_wasm(err);
            return CallOutcome::error(tag, payload);
        }
    };
    let instance = match linker.instantiate(&mut store, &pc.module) {
        Ok(i) => i,
        Err(e) => {
            let heap = unsafe { &mut *caller.data().heap_ptr() };
            let ctx = crate::primitives::ctx::Alloc::new(heap);
            let err = ctx.error("internal-error", e.to_string());
            let (tag, payload) = caller.data_mut().value_to_wasm(err);
            return CallOutcome::error(tag, payload);
        }
    };

    // Build env in the standalone module's linear memory.
    // Reuse the same layout as prepare_wasm_env: [captures][params][locals]
    let memory = instance
        .get_memory(&mut store, "__elle_memory")
        .expect("precached closure: no memory");
    // Start above this standalone closure's widest args region so a wide call in
    // its body cannot clobber its own env.
    store.data_mut().env_stack_ptr = pc.env_stack_base;
    let env_base = pc.env_stack_base;
    build_env_in_store(&mut store, &memory, closure, args, env_base);

    // Install the executing closure in this fresh store's self slot (converted into
    // its own handle space), so a `LoadSelf` in the body — and any self-tail-call or
    // suspend/resume, which run on this store — resolve to it. Fresh store, so no
    // save/restore: the slot has no prior tenant.
    let (self_tag, self_payload) = store.data_mut().value_to_wasm(self_val);
    write_self_slot(&mut store, &memory, self_tag, self_payload);

    // Call the closure function (exported as __elle_closure)
    let func = match instance
        .get_typed_func::<(i32, i32, i32, i32), (i64, i64, i64)>(&mut store, "__elle_closure")
    {
        Ok(f) => f,
        Err(e) => {
            let heap = unsafe { &mut *caller.data().heap_ptr() };
            let ctx = crate::primitives::ctx::Alloc::new(heap);
            let err = ctx.error("internal-error", e.to_string());
            let (tag, payload) = caller.data_mut().value_to_wasm(err);
            return CallOutcome::error(tag, payload);
        }
    };

    match func.call(&mut store, (env_base as i32, 0, 0, 0)) {
        Ok((tag, payload, status)) => {
            // A precached closure cannot suspend — precaching only engages when
            // no closure in the module may suspend, and `standalone_emittable`
            // refuses every parking shape besides — so a non-zero status means
            // the emission gate and this caller have drifted apart.
            if status != 0 {
                let heap = unsafe { &mut *caller.data().heap_ptr() };
                let ctx = crate::primitives::ctx::Alloc::new(heap);
                let err = ctx.error(
                    "internal-error",
                    format!(
                        "precached closure suspended at resume state {status}, \
                         which a standalone store cannot drive"
                    ),
                );
                let (tag, payload) = caller.data_mut().value_to_wasm(err);
                return CallOutcome::error(tag, payload);
            }
            let signal = take_raised_signal(&mut store, &memory);
            // Convert result from standalone store's handle space
            // back to the caller's handle space.
            let value = store.data().wasm_to_value(tag, payload);
            let (caller_tag, caller_payload) = caller.data_mut().value_to_wasm(value);
            CallOutcome::signalled(caller_tag, caller_payload, signal)
        }
        Err(e) => {
            let heap = unsafe { &mut *caller.data().heap_ptr() };
            let ctx = crate::primitives::ctx::Alloc::new(heap);
            let err = ctx.error("internal-error", e.to_string());
            let (tag, payload) = caller.data_mut().value_to_wasm(err);
            CallOutcome::error(tag, payload)
        }
    }
}
