//! Constant-pool and closure-construction host functions:
//! `rt_load_const` and `rt_make_closure`.

use wasmtime::*;

use super::{Value, TAG_HEAP_START};
use crate::wasm::host::ElleHost;

pub(super) fn register(linker: &mut Linker<ElleHost>) -> Result<()> {
    // rt_load_const(index: i32) -> (tag: i64, payload: i64)
    linker.func_wrap(
        "elle",
        "rt_load_const",
        |caller: Caller<'_, ElleHost>, index: i32| -> (i64, i64) {
            let host = caller.data();
            let value = host.const_pool[index as usize];

            if value.tag < TAG_HEAP_START {
                (value.tag as i64, value.payload as i64)
            } else {
                // Heap value — use pre-computed handle from create_store.
                let handle = host.pool_to_handle[index as usize];
                (value.tag as i64, handle as i64)
            }
        },
    )?;

    // rt_make_closure(table_idx: i32, captures_ptr: i32, metadata_ptr: i32) -> (tag: i64, payload: i64)
    linker.func_wrap(
        "elle",
        "rt_make_closure",
        |mut caller: Caller<'_, ElleHost>,
         table_idx: i32,
         captures_ptr: i32,
         metadata_ptr: i32|
         -> (i64, i64) {
            // Read metadata from linear memory
            let memory = caller
                .get_export("__elle_memory")
                .and_then(|e| e.into_memory())
                .expect("rt_make_closure: no memory");
            let data = memory.data(&caller);
            let read_i64 = |offset: usize| -> i64 {
                i64::from_le_bytes(data[offset..offset + 8].try_into().unwrap())
            };
            let mp = metadata_ptr as usize;
            let num_captures = read_i64(mp) as u16;
            let num_params = read_i64(mp + 8) as usize;
            let num_locals = read_i64(mp + 16) as usize;
            let arity_kind = read_i64(mp + 24);
            let arity_count = read_i64(mp + 32) as usize;
            let capture_params_mask = read_i64(mp + 40) as u64;
            // Slot 6 is the word count of the unbounded locals mask; the words
            // follow the 8 fixed slots (written in `emit_make_closure`,
            // src/wasm/instruction.rs).
            let locals_mask_nwords = read_i64(mp + 48) as usize;
            let signal_bits = read_i64(mp + 56) as u64;
            let capture_locals_mask = crate::value::CaptureMask::from_words(
                (0..locals_mask_nwords)
                    .map(|j| read_i64(mp + 64 + j * 8) as u64)
                    .collect(),
            );

            // Read captures from linear memory
            let mut captures = Vec::with_capacity(num_captures as usize);
            for i in 0..num_captures as usize {
                let offset = captures_ptr as usize + i * 16;
                let tag = read_i64(offset) as u64;
                let payload = read_i64(offset + 8) as u64;
                let value = if tag < TAG_HEAP_START {
                    Value { tag, payload }
                } else {
                    caller.data().handles.get(payload)
                };
                captures.push(value);
            }

            let arity = match arity_kind {
                0 => crate::value::types::Arity::Exact(arity_count),
                1 => crate::value::types::Arity::AtLeast(arity_count),
                _ => crate::value::types::Arity::Exact(arity_count),
            };

            // Create a ClosureTemplate with wasm_func_idx.
            // Also populate bytecode from dual-compiled closures so spawn works.
            // child_protos are the nested-lambda blueprints the bytecode's
            // MakeClosure instructions index; an OS-thread VM worker running this
            // template needs them, so they ride alongside bytecode + constants.
            let (bytecode, constants, child_protos) = caller
                .data()
                .closure_bytecodes
                .get(table_idx as usize)
                .map(|(bc, cs, cp)| (bc.clone(), cs.clone(), cp.clone()))
                .unwrap_or_else(|| {
                    (
                        std::rc::Rc::new(vec![]),
                        std::rc::Rc::new(vec![]),
                        std::rc::Rc::new(vec![]),
                    )
                });
            let template = std::rc::Rc::new(crate::value::closure::ClosureTemplate {
                num_locals,
                num_captures: num_captures as usize,
                num_params,
                signal: crate::signals::Signal {
                    bits: crate::value::fiber::SignalBits::new(signal_bits),
                    propagates: 0,
                },
                capture_params_mask,
                capture_locals_mask,
                wasm_func_idx: Some(table_idx as u32),
                child_protos,
                ..crate::value::closure::ClosureTemplate::new(bytecode, arity, constants)
            });

            // Build the closure + its captured-env slice through a boundary ctx
            // over its own fresh result region.
            let heap = unsafe { &mut *caller.data().heap_ptr() };
            let ctx = crate::primitives::ctx::Alloc::new(heap);
            let closure = crate::value::closure::Closure {
                template: crate::value::TemplateRef::new(template),
                env: ctx.alloc_slice::<Value>(&captures),
                squelch_mask: crate::value::fiber::SignalBits::EMPTY,
            };

            let value = ctx.closure(closure);
            let (tag, payload) = caller.data_mut().value_to_wasm(value);
            (tag, payload)
        },
    )?;

    Ok(())
}
