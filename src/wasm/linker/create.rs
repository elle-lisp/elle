use super::*;

/// Register host functions and return a Linker.
pub fn create_linker(engine: &Engine) -> Result<Linker<ElleHost>> {
    let mut linker = Linker::new(engine);

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

    // rt_call(func_tag: i64, func_payload: i64, args_ptr: i32, nargs: i32, ctx: i32) -> (tag: i64, payload: i64, signal: i64)
    linker.func_wrap(
        "elle",
        "rt_call",
        |mut caller: Caller<'_, ElleHost>,
         func_tag: i64,
         func_payload: i64,
         args_ptr: i32,
         nargs: i32,
         _ctx: i32|
         -> (i64, i64, i64) {
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

                // Handle SIG_RESUME: fiber/resume returns this signal.
                // Execute the fiber's WASM closure host-side.
                if bits.raw() & 8 != 0 {
                    // SIG_RESUME: result is the fiber value
                    let r = super::super::resume::handle_fiber_resume(&mut caller, result);
                    if caller.data().debug {
                        eprintln!(
                            "[rt_call] handle_fiber_resume returned: tag={} payload={} signal={}",
                            r.0, r.1, r.2
                        );
                    }
                    return r;
                }

                let (tag, payload) = caller.data_mut().value_to_wasm(result);
                (tag, payload, bits.raw() as i64)
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
                    (tag, payload, 1)
                } else {
                    let value = caller.data().resolve_parameter(id, default);
                    let (tag, payload) = caller.data_mut().value_to_wasm(value);
                    (tag, payload, 0)
                }
            } else if let Some(closure) = func_val.as_closure() {
                if let Some(wasm_idx) = closure.template.wasm_func_idx {
                    // Check for pre-compiled per-closure Module first.
                    let precached = caller
                        .data()
                        .precached_closures
                        .get(wasm_idx as usize)
                        .and_then(|opt| opt.as_ref())
                        .cloned();
                    if let Some(ref pc) = precached {
                        super::super::store::call_precached_closure(
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
                        super::super::store::call_wasm_closure(
                            &mut caller,
                            closure,
                            wasm_idx,
                            &args,
                            func_tag,
                            func_payload,
                        )
                    }
                } else {
                    // Bytecode closure — not supported in WASM backend
                    let heap = unsafe { &mut *caller.data().heap_ptr() };
                    let ctx = crate::primitives::ctx::Alloc::new(heap);
                    let err = ctx.error(
                        "internal-error",
                        "rt_call: bytecode closure in WASM backend",
                    );
                    let (tag, payload) = caller.data_mut().value_to_wasm(err);
                    (tag, payload, 1)
                }
            } else {
                let heap = unsafe { &mut *caller.data().heap_ptr() };
                let ctx = crate::primitives::ctx::Alloc::new(heap);
                let err = ctx.error(
                    "type-error",
                    format!("rt_call: cannot call {}", func_val.type_name()),
                );
                let (tag, payload) = caller.data_mut().value_to_wasm(err);
                (tag, payload, 1)
            }
        },
    )?;

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
            let (bytecode, constants) = caller
                .data()
                .closure_bytecodes
                .get(table_idx as usize)
                .map(|(bc, cs)| (bc.clone(), cs.clone()))
                .unwrap_or_else(|| (std::rc::Rc::new(vec![]), std::rc::Rc::new(vec![])));
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

    // rt_data_op(op: i32, args_ptr: i32, nargs: i32) -> (tag: i64, payload: i64, signal: i64)
    linker.func_wrap(
        "elle",
        "rt_data_op",
        |mut caller: Caller<'_, ElleHost>, op: i32, args_ptr: i32, nargs: i32| -> (i64, i64, i64) {
            let args = read_args_from_memory(&mut caller, args_ptr, nargs);
            let vm = caller.data().vm;
            let (bits, result) = dispatch_data_op(op, &args, vm);
            let (tag, payload) = caller.data_mut().value_to_wasm(result);
            (tag, payload, bits.raw() as i64)
        },
    )?;

    // rt_push_param(args_ptr: i32, npairs: i32) -> ()
    linker.func_wrap(
        "elle",
        "rt_push_param",
        |mut caller: Caller<'_, ElleHost>, args_ptr: i32, npairs: i32| {
            let memory = caller
                .get_export("__elle_memory")
                .and_then(|e| e.into_memory())
                .expect("rt_push_param: no memory");

            // Read (param, value) pairs from linear memory.
            // Each pair is 32 bytes: param(tag,payload) + value(tag,payload).
            let mut frame = Vec::with_capacity(npairs as usize);
            for i in 0..npairs as usize {
                let base = args_ptr as usize + i * 32;
                let data = memory.data(&caller);
                let read_i64 = |offset: usize| -> i64 {
                    i64::from_le_bytes(data[offset..offset + 8].try_into().unwrap())
                };
                let param_tag = read_i64(base) as u64;
                let param_payload = read_i64(base + 8) as u64;
                let val_tag = read_i64(base + 16) as u64;
                let val_payload = read_i64(base + 24) as u64;

                // Resolve param value from handle table
                let param_val = caller
                    .data()
                    .wasm_to_value(param_tag as i64, param_payload as i64);
                let value = caller
                    .data()
                    .wasm_to_value(val_tag as i64, val_payload as i64);

                // Extract parameter id
                if let Some((id, _)) = param_val.as_parameter() {
                    frame.push((id, value));
                }
            }
            caller.data_mut().param_frames.push(frame);
        },
    )?;

    // rt_pop_param() -> ()
    linker.func_wrap(
        "elle",
        "rt_pop_param",
        |mut caller: Caller<'_, ElleHost>| {
            caller.data_mut().param_frames.pop();
        },
    )?;

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
                    super::super::store::prepare_wasm_env(&mut caller, closure, &args, env_base);

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
                    super::super::store::write_self_slot(
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
                        super::super::resume::handle_fiber_resume(&mut caller, result);
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

    // rt_yield(tag: i64, payload: i64, resume_state: i32, regs_ptr: i32, num_regs: i32, func_idx: i32, signal_bits: i64)
    // Save yielded value and live registers to a WasmSuspensionFrame.
    linker.func_wrap(
        "elle",
        "rt_yield",
        |mut caller: Caller<'_, ElleHost>,
         tag: i64,
         payload: i64,
         resume_state: i32,
         regs_ptr: i32,
         num_regs: i32,
         func_idx: i32,
         signal_bits: i64| {
            // Read saved registers from linear memory
            let saved_regs = read_reg_pairs(&mut caller, regs_ptr, num_regs);

            // Snapshot the executing closure so resume restores it (the store's self
            // slot is shared, so an interleaved fiber may overwrite it before resume).
            let self_memory = caller
                .get_export("__elle_memory")
                .and_then(|e| e.into_memory())
                .expect("rt_yield: no memory");
            let (self_tag, self_payload) = super::super::store::read_self_slot(&caller, &self_memory);

            if caller.data().debug {
                eprintln!(
                    "[rt_yield] tag={} payload={} resume_state={} num_regs={} func_idx={} signal_bits={}",
                    tag, payload, resume_state, num_regs, func_idx, signal_bits
                );
            }

            let host = caller.data_mut();
            host.push_suspension_frame(super::super::host::WasmSuspensionFrame {
                wasm_func_idx: func_idx as u32,
                resume_state: resume_state as u32,
                saved_regs,
                env_snapshot: Vec::new(),
                env_base: 0,
                signal_bits: signal_bits as u64,
                self_tag,
                self_payload,
            });
        },
    )?;

    // rt_get_resume_value() -> (tag: i64, payload: i64)
    // Return the resume value set by the scheduler.
    linker.func_wrap(
        "elle",
        "rt_get_resume_value",
        |caller: Caller<'_, ElleHost>| -> (i64, i64) {
            let host = caller.data();
            let result = match host.resume_value {
                Some((tag, payload)) => (tag, payload),
                None => (crate::value::repr::TAG_NIL as i64, 0),
            };
            if caller.data().debug {
                eprintln!(
                    "[rt_get_resume_value] tag={} payload={} (resume_value={:?})",
                    result.0,
                    result.1,
                    host.resume_value.is_some()
                );
            }
            result
        },
    )?;

    // rt_load_saved_reg(index: i32) -> (tag: i64, payload: i64)
    // Load a saved register by index from the current suspension frame.
    linker.func_wrap(
        "elle",
        "rt_load_saved_reg",
        |caller: Caller<'_, ElleHost>, index: i32| -> (i64, i64) {
            let host = caller.data();
            // The front frame is always the one being resumed (innermost).
            // New frames pushed by rt_yield during the call go to the back.
            let frame_ref = host.first_suspension_frame();
            if let Some(frame) = frame_ref {
                if (index as usize) < frame.saved_regs.len() {
                    let (tag, pay) = frame.saved_regs[index as usize];
                    if caller.data().debug && index < 5 {
                        eprintln!(
                            "[rt_load_saved_reg] index={} tag={} payload={} (frame has {} regs)",
                            index,
                            tag,
                            pay,
                            frame.saved_regs.len()
                        );
                    }
                    (tag, pay)
                } else {
                    (crate::value::repr::TAG_NIL as i64, 0)
                }
            } else {
                if caller.data().debug {
                    eprintln!("[rt_load_saved_reg] NO FRAME! index={}", index);
                }
                (crate::value::repr::TAG_NIL as i64, 0)
            }
        },
    )?;

    Ok(linker)
}
