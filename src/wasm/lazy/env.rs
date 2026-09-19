// audited: 2026-09-19
//! Building a tiered closure's environment in WASM linear memory.
//!
//! docs/impl/wasm.md

use super::*;

/// Build closure environment in WASM linear memory.
///
/// Layout: [captures...] [params...] [local_slots(zeroed)...]
/// Each slot is 16 bytes (tag: i64, payload: i64).
/// Follows the same pattern as `prepare_wasm_env` in store.rs:
/// interleaves `value_to_wasm` with `data_mut` to avoid borrow issues.
pub(super) fn build_env_in_memory(
    store: &mut Store<TieredHost>,
    memory: &Memory,
    closure: &crate::value::closure::Closure,
    args: &[Value],
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
    let env_heap_ptr = unsafe { (*store.data().vm).heap_ptr };
    let fresh_region = move || unsafe { (*env_heap_ptr).new_runtime_region() };

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
        let (tag, payload) = store.data_mut().inner.value_to_wasm(val);
        let offset = env_base + (num_captures + i) * 16;
        let data = memory.data_mut(&mut *store);
        data[offset..offset + 8].copy_from_slice(&tag.to_le_bytes());
        data[offset + 8..offset + 16].copy_from_slice(&payload.to_le_bytes());
    }

    // Write nil for remaining params
    for i in args.len()..num_params {
        let val = if i < 64 && capture_params_mask & (1u64 << i) != 0 {
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
        let (tag, payload) = store.data_mut().inner.value_to_wasm(val);
        let offset = env_base + (num_captures + i) * 16;
        let data = memory.data_mut(&mut *store);
        data[offset..offset + 8].copy_from_slice(&tag.to_le_bytes());
        data[offset + 8..offset + 16].copy_from_slice(&payload.to_le_bytes());
    }

    // Write the extra local slots: nil, or a capture cell holding nil.
    // Precise at any index: a captured local is celled, an uncaptured one
    // (even >= 64) gets bare NIL.
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
        let (tag, payload) = store.data_mut().inner.value_to_wasm(val);
        let offset = env_base + (num_captures + num_params + i) * 16;
        let data = memory.data_mut(&mut *store);
        data[offset..offset + 8].copy_from_slice(&tag.to_le_bytes());
        data[offset + 8..offset + 16].copy_from_slice(&payload.to_le_bytes());
    }
}
