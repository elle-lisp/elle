//! Primary call-dispatch host functions: `call_primitive` and `rt_call`.
//!
//! Grouped together because both resolve a callable and drive it host-side,
//! sharing the fiber-resume (`SIG_RESUME`) handoff to `resume::handle_fiber_resume`.

use wasmtime::*;

use crate::wasm::host::ElleHost;
use crate::wasm::linker::read_args_from_memory;
use crate::wasm::outcome::CallOutcome;

pub(super) fn register(linker: &mut Linker<ElleHost>) -> Result<()> {
    // call_primitive(prim_id: i32, args_ptr: i32, nargs: i32, ctx: i32) -> (tag: i64, payload: i64, signal: i64)
    linker.func_wrap(
        "elle",
        "call_primitive",
        |mut caller: Caller<'_, ElleHost>,
         prim_id: i32,
         args_ptr: i32,
         nargs: i32,
         _ctx: i32|
         -> (i64, i64, i64) {
            let args = read_args_from_memory(&mut caller, args_ptr, nargs);
            let (bits, result) = caller.data_mut().call_primitive(prim_id as u32, &args);
            let (bits, result) = caller.data_mut().maybe_execute_io(bits, result);
            let (tag, payload) = caller.data_mut().value_to_wasm(result);
            (tag, payload, bits.raw() as i64)
        },
    )?;

    // rt_call(func_tag: i64, func_payload: i64, args_ptr: i32, nargs: i32, ctx: i32)
    //   -> (tag: i64, payload: i64, signal: i64, suspended: i64)
    //
    // `suspended` is the word emitted code branches on. It is computed here, once,
    // by `signals::dispatch::is_suspending` — never inferred from a bit of
    // `signal`. See docs/impl/wasm.md § rt_call.
    linker.func_wrap(
        "elle",
        "rt_call",
        |mut caller: Caller<'_, ElleHost>,
         func_tag: i64,
         func_payload: i64,
         args_ptr: i32,
         nargs: i32,
         _ctx: i32|
         -> (i64, i64, i64, i64) {
            // Resolve the function value
            let func_val = caller.data().wasm_to_value(func_tag, func_payload);

            // Read args from linear memory.
            // nargs=-1 is the CallArrayMut protocol: the args array is
            // at args_ptr + 16 (slot 1). Unpack it into a flat arg list.
            if caller.data().debug {
                eprintln!("[rt_call] type={} nargs={}", func_val.type_name(), nargs);
            }
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

            // Dispatch based on function type
            if func_val.is_native_fn() {
                let native_fn = func_val.as_native_fn().expect("rt_call: expected NativeFn");
                if caller.data().debug && nargs == 2 {
                    eprintln!(
                        "[rt_call] native 2args: [{}, {}]",
                        args[0].type_name(),
                        args[1].type_name()
                    );
                }
                let vm = caller.data().vm;
                let heap = unsafe { &mut *caller.data().heap_ptr() };
                let region = heap.new_runtime_region();
                let mut ctx = crate::primitives::ctx::NativeCtx::with_region_vm(region, heap, vm);
                let (bits, result) = native_fn(&mut ctx, &args);
                if caller.data().debug && bits.raw() != 0 {
                    eprintln!(
                        "[rt_call] native returned signal={} value={:?}",
                        bits.raw(),
                        result
                    );
                }
                let (bits, result) = caller.data_mut().maybe_execute_io(bits, result);

                // Handle SIG_PROPAGATE: fiber/propagate re-raises a child's caught
                // signal. Convert it to the child's (bits, value) so the body
                // unwinds with the real error (defer/with's tail unwind branch).
                if bits.raw() & crate::value::fiber::SIG_PROPAGATE.raw() != 0 {
                    return crate::wasm::resume::handle_fiber_propagate(&mut caller, result)
                        .to_wasm();
                }

                // Handle SIG_RESUME: fiber/resume returns this signal.
                // Execute the fiber's WASM closure host-side.
                if bits.raw() & 8 != 0 {
                    // SIG_RESUME: result is the fiber value
                    let r = crate::wasm::resume::handle_fiber_resume(&mut caller, result);
                    if caller.data().debug {
                        eprintln!(
                            "[rt_call] handle_fiber_resume returned: tag={} payload={} signal={} suspended={}",
                            r.tag, r.payload, r.signal, r.suspended
                        );
                    }
                    return r.to_wasm();
                }

                let (tag, payload) = caller.data_mut().value_to_wasm(result);
                CallOutcome::signalled(tag, payload, bits)
            } else if let Some((id, default)) = func_val.as_parameter() {
                if caller.data().debug {
                    eprintln!("[rt_call] parameter id={} default={:?}", id, default);
                }
                if !args.is_empty() {
                    let heap = unsafe { &mut *caller.data().heap_ptr() };
                    let ctx = crate::primitives::ctx::Alloc::new(heap);
                    let err = ctx.error(
                        "arity-error",
                        format!("parameter call: expected 0 arguments, got {}", args.len()),
                    );
                    let (tag, payload) = caller.data_mut().value_to_wasm(err);
                    CallOutcome::error(tag, payload)
                } else {
                    let value = caller.data().resolve_parameter(id, default);
                    let (tag, payload) = caller.data_mut().value_to_wasm(value);
                    CallOutcome::value(tag, payload)
                }
            } else if let Some(closure) = func_val.as_closure() {
                if let Some(wasm_idx) = closure.template.wasm_func_idx() {
                    // Check for pre-compiled per-closure Module first.
                    let precached = caller
                        .data()
                        .precached_closures
                        .get(wasm_idx as usize)
                        .and_then(|opt| opt.as_ref())
                        .cloned();
                    if let Some(ref pc) = precached {
                        crate::wasm::store::call_precached_closure(
                            &mut caller,
                            closure,
                            pc,
                            &args,
                            func_val,
                        )
                    } else {
                        // Fall back to full module's table. The callee closure is the
                        // executing self for the body's `LoadSelf` — pass its (tag,
                        // payload) so `call_wasm_closure` installs and restores it.
                        crate::wasm::store::call_wasm_closure(
                            &mut caller,
                            closure,
                            wasm_idx,
                            &args,
                            func_tag,
                            func_payload,
                        )
                    }
                } else {
                    // Bytecode closure (core.lisp / prelude / a runtime closure the
                    // module never compiled) — execute it via the host VM. See
                    // `crate::wasm::linker::run_bytecode_closure`.
                    crate::wasm::linker::run_bytecode_closure(&mut caller, closure, func_val, &args)
                }
            } else {
                // A callable collection (struct/array/set/string/bytes indexed
                // by a key, e.g. `(request :op)`) — or, failing that, the
                // `cannot call` type error. See `run_collection_call`.
                crate::wasm::linker::run_collection_call(&mut caller, func_val, &args, "rt_call")
            }
            .to_wasm()
        },
    )?;

    Ok(())
}
