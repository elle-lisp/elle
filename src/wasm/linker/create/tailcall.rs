//! Tail-call preparation host function: `rt_prepare_tail_call`.
//!
//! Isolated because it owns the env-stack reset / rebuild dance and the
//! tail-position fiber-resume handling that distinguishes it from `rt_call`.

use wasmtime::*;

use crate::wasm::host::ElleHost;
use crate::wasm::linker::read_args_from_memory;

pub(super) fn register(linker: &mut Linker<ElleHost>) -> Result<()> {
    // rt_prepare_tail_call(func_tag, func_payload, args_ptr, nargs, caller_env_ptr)
    //   -> (env_ptr, table_idx, is_wasm, tag, payload, signal)
    //
    // Prepares a tail call: resolves the target, builds env if WASM closure,
    // or calls directly if NativeFn/Parameter. Returns enough info for the
    // WASM caller to either `return_call_indirect` or `return` the result.
    linker.func_wrap(
        "elle",
        "rt_prepare_tail_call",
        |mut caller: Caller<'_, ElleHost>,
         func_tag: i64,
         func_payload: i64,
         args_ptr: i32,
         nargs: i32,
         caller_env_ptr: i32|
         -> (i32, i32, i32, i64, i64, i64) {
            let func_val = caller.data().wasm_to_value(func_tag, func_payload);

            if caller.data().debug {
                let args_debug = read_args_from_memory(&mut caller, args_ptr, nargs);
                eprintln!(
                    "[rt_prepare_tail_call] type={} nargs={} args={:?}",
                    func_val.type_name(),
                    nargs,
                    args_debug
                        .iter()
                        .map(|v| format!("{}", v))
                        .collect::<Vec<_>>()
                );
            }

            // Read args (same protocol as rt_call: nargs=-1 unpacks array)
            let args = if nargs == -1 {
                let raw = read_args_from_memory(&mut caller, args_ptr + 16, 1);
                if let Some(arr) = raw[0].as_array_mut() {
                    arr.borrow().to_vec()
                } else if let Some(arr) = raw[0].as_array() {
                    arr.to_vec()
                } else {
                    vec![raw[0]]
                }
            } else {
                read_args_from_memory(&mut caller, args_ptr, nargs)
            };

            if let Some(closure) = func_val.as_closure() {
                if let Some(wasm_idx) = closure.template.wasm_func_idx {
                    // Reset env_stack_ptr to caller's position (frees caller's env)
                    let env_base = caller_env_ptr as usize;
                    caller.data_mut().env_stack_ptr = env_base;
                    // Build callee's env at the same position
                    crate::wasm::store::prepare_wasm_env(&mut caller, closure, &args, env_base);

                    if caller.data().debug {
                        let env_end = caller.data().env_stack_ptr;
                        let memory = caller
                            .get_export("__elle_memory")
                            .and_then(|e| e.into_memory())
                            .expect("debug");
                        let data = memory.data(&caller);
                        let num_slots = (env_end - env_base) / 16;
                        let mut slots = Vec::new();
                        for i in 0..num_slots.min(5) {
                            let off = env_base + i * 16;
                            let t = i64::from_le_bytes(data[off..off + 8].try_into().unwrap());
                            let p = i64::from_le_bytes(data[off + 8..off + 16].try_into().unwrap());
                            slots.push(format!("({},{})", t, p));
                        }
                        eprintln!(
                            "[rt_prepare_tail_call] env after prepare: base={} end={} slots={:?}",
                            env_base, env_end, slots
                        );
                    }

                    // The tail-called closure is the executing self for the body it
                    // re-enters via `return_call_indirect` (shared linear memory). No
                    // restore: a tail call replaces the frame, so the caller never
                    // resumes to observe its own self again.
                    let self_memory = caller
                        .get_export("__elle_memory")
                        .and_then(|e| e.into_memory())
                        .expect("rt_prepare_tail_call: no memory");
                    crate::wasm::store::write_self_slot(
                        &mut caller,
                        &self_memory,
                        func_tag,
                        func_payload,
                    );
                    return (env_base as i32, wasm_idx as i32, 1, 0, 0, 0);
                }
                let heap = unsafe { &mut *caller.data().heap_ptr() };
                let ctx = crate::primitives::ctx::Alloc::new(heap);
                let err = ctx.error(
                    "internal-error",
                    "rt_prepare_tail_call: bytecode closure in WASM backend",
                );
                let (tag, payload) = caller.data_mut().value_to_wasm(err);
                return (0, 0, 0, tag, payload, 1);
            }

            if func_val.is_native_fn() {
                let native_fn = func_val
                    .as_native_fn()
                    .expect("rt_prepare_tail_call: expected NativeFn");
                let vm = caller.data().vm;
                let heap = unsafe { &mut *caller.data().heap_ptr() };
                let region = heap.new_runtime_region();
                let mut ctx = crate::primitives::ctx::NativeCtx::with_region_vm(region, heap, vm);
                let (bits, result) = native_fn(&mut ctx, &args);
                let (bits, result) = caller.data_mut().maybe_execute_io(bits, result);
                // A tail-position `(fiber/resume …)` returns SIG_RESUME just like a
                // non-tail one; drive the fiber host-side here exactly as `rt_call`
                // does. Without this the tail call returns the fiber itself
                // ("<fiber:new>") — the resume never runs. `handle_fiber_resume`
                // yields (tag, payload, signal); route a non-zero signal through
                // memory[0..8] like the native-error path below so the caller's
                // epilogue observes it (tail dispatch returns tag/payload/0).
                if bits.raw() & crate::value::fiber::SIG_RESUME.raw() != 0 {
                    let (tag, payload, signal) =
                        crate::wasm::resume::handle_fiber_resume(&mut caller, result);
                    if signal != 0 {
                        if let Some(memory) = caller
                            .get_export("__elle_memory")
                            .and_then(|e| e.into_memory())
                        {
                            memory.data_mut(&mut caller)[0..8]
                                .copy_from_slice(&signal.to_le_bytes());
                        }
                    }
                    return (0, 0, 0, tag, payload, signal);
                }
                // Write non-zero signal to memory[0..8] so handle_wasm_result
                // picks it up. The WASM tail call dispatch returns immediately
                // after this host call (just tag/payload/0), so no WASM code
                // overwrites memory[0..8] before the function exits.
                if bits.raw() != 0 {
                    if let Some(memory) = caller
                        .get_export("__elle_memory")
                        .and_then(|e| e.into_memory())
                    {
                        memory.data_mut(&mut caller)[0..8]
                            .copy_from_slice(&(bits.raw() as i64).to_le_bytes());
                    }
                }
                let (tag, payload) = caller.data_mut().value_to_wasm(result);
                return (0, 0, 0, tag, payload, bits.raw() as i64);
            }

            if let Some((id, default)) = func_val.as_parameter() {
                if !args.is_empty() {
                    let heap = unsafe { &mut *caller.data().heap_ptr() };
                    let ctx = crate::primitives::ctx::Alloc::new(heap);
                    let err = ctx.error(
                        "arity-error",
                        format!("parameter call: expected 0 arguments, got {}", args.len()),
                    );
                    let (tag, payload) = caller.data_mut().value_to_wasm(err);
                    return (0, 0, 0, tag, payload, 1);
                }
                let value = caller.data().resolve_parameter(id, default);
                let (tag, payload) = caller.data_mut().value_to_wasm(value);
                return (0, 0, 0, tag, payload, 0);
            }

            let heap = unsafe { &mut *caller.data().heap_ptr() };
            let ctx = crate::primitives::ctx::Alloc::new(heap);
            let err = ctx.error(
                "type-error",
                format!("rt_prepare_tail_call: cannot call {}", func_val.type_name()),
            );
            let (tag, payload) = caller.data_mut().value_to_wasm(err);
            (0, 0, 0, tag, payload, 1)
        },
    )?;

    Ok(())
}
