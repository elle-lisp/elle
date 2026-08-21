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
    let num_captures = template.num_captures;
    let num_params = template.num_params;
    let num_locals = template.num_locals;
    let capture_params_mask = template.capture_params_mask;
    let capture_locals_mask = &template.capture_locals_mask;
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

    // Write params with optional LBox wrapping
    for (i, arg) in args.iter().enumerate().take(num_params) {
        let val = if i < 64 && capture_params_mask & (1u64 << i) != 0 {
            let region = fresh_region();
            crate::value::build::capture_cell(unsafe { &mut *env_heap_ptr }, *arg, region)
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
            crate::value::build::capture_cell(unsafe { &mut *env_heap_ptr }, Value::NIL, region)
        } else {
            Value::NIL
        };
        let (tag, payload) = store.data_mut().inner.value_to_wasm(val);
        let offset = env_base + (num_captures + i) * 16;
        let data = memory.data_mut(&mut *store);
        data[offset..offset + 8].copy_from_slice(&tag.to_le_bytes());
        data[offset + 8..offset + 16].copy_from_slice(&payload.to_le_bytes());
    }

    // Write nil/LBox(nil) for extra local slots. Precise at any index: a
    // captured local is celled, an uncaptured one (even >= 64) gets bare NIL.
    for i in 0..extra_locals {
        let val = if capture_locals_mask.is_set(i) {
            let region = fresh_region();
            crate::value::build::capture_cell(unsafe { &mut *env_heap_ptr }, Value::NIL, region)
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
pub(super) fn create_tiered_linker(engine: &Engine) -> Result<Linker<TieredHost>> {
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
         -> (i64, i64, i64) {
            let args = read_args(&mut caller, args_ptr, nargs);
            let (bits, result) = caller
                .data_mut()
                .inner
                .call_primitive(prim_id as u32, &args);
            let (bits, result) = caller.data_mut().inner.maybe_execute_io(bits, result);
            let (tag, payload) = caller.data_mut().inner.value_to_wasm(result);
            (tag, payload, bits.raw() as i64)
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
         -> (i64, i64, i64) {
            let func_val = caller.data().inner.wasm_to_value(func_tag, func_payload);
            let args = read_args(&mut caller, args_ptr, nargs);

            if func_val.is_native_fn() {
                let native_fn = func_val.as_native_fn().unwrap();
                let vm = caller.data().vm;
                let heap = unsafe { &mut *(*caller.data().vm).heap_ptr };
                let region = heap.new_runtime_region();
                let mut ctx = crate::primitives::ctx::NativeCtx::with_region_vm(region, heap, vm);
                let (bits, result) = native_fn(&mut ctx, &args);
                let (bits, result) = caller.data_mut().inner.maybe_execute_io(bits, result);
                let (tag, payload) = caller.data_mut().inner.value_to_wasm(result);
                return (tag, payload, bits.raw() as i64);
            }

            if let Some((id, default)) = func_val.as_parameter() {
                if !args.is_empty() {
                    let heap = unsafe { &mut *(*caller.data().vm).heap_ptr };
                    let ctx = crate::primitives::ctx::Alloc::new(heap);
                    let err = ctx.error(
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
                    super::super::store::prepare_wasm_env(&mut caller, closure, &args, env_base);

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

                    // Install this closure as the executing self, restoring the
                    // caller's on return (shared linear memory). It IS a self-call, so
                    // the value is the same, but the save/restore keeps the slot honest
                    // for a caller that was a different closure.
                    let self_memory = caller
                        .get_export("__elle_memory")
                        .and_then(|e| e.into_memory())
                        .expect("rt_call self-dispatch: no memory");
                    let saved_self = super::super::store::read_self_slot(&caller, &self_memory);
                    super::super::store::write_self_slot(
                        &mut caller,
                        &self_memory,
                        func_tag,
                        func_payload,
                    );

                    let mut results = [Val::I64(0), Val::I64(0), Val::I64(0)];
                    let call_result = func.call(
                        &mut caller,
                        &[
                            Val::I32(env_base as i32),
                            Val::I32(0),
                            Val::I32(0),
                            Val::I32(0),
                        ],
                        &mut results,
                    );
                    super::super::store::write_self_slot(
                        &mut caller,
                        &self_memory,
                        saved_self.0,
                        saved_self.1,
                    );
                    match call_result {
                        Ok(()) => {
                            let tag = results[0].unwrap_i64();
                            let payload = results[1].unwrap_i64();
                            let status = results[2].unwrap_i64();
                            // Restore env_stack_ptr after the call
                            caller.data_mut().inner.env_stack_ptr = env_base;
                            return (tag, payload, status);
                        }
                        Err(e) => {
                            let heap = unsafe { &mut *(*caller.data().vm).heap_ptr };
                            let ctx = crate::primitives::ctx::Alloc::new(heap);
                            let err = ctx.error("internal-error", format!("wasm self-call: {}", e));
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
                    match wasm_tier.call(vm, bytecode_ptr, &closure_rc, &args, func_val) {
                        Ok((value, signal)) => {
                            if signal.is_empty() {
                                let (tag, payload) = caller.data_mut().inner.value_to_wasm(value);
                                return (tag, payload, 0);
                            } else if signal == crate::value::SIG_HALT {
                                if value == Value::NIL {
                                    let (tag, payload) =
                                        caller.data_mut().inner.value_to_wasm(value);
                                    return (tag, payload, 0);
                                }
                                // Non-NIL halt: propagate as error
                                let (tag, payload) = caller.data_mut().inner.value_to_wasm(value);
                                return (tag, payload, signal.raw() as i64);
                            }
                            let (tag, payload) = caller.data_mut().inner.value_to_wasm(value);
                            return (tag, payload, signal.raw() as i64);
                        }
                        Err(e) => {
                            let heap = unsafe { &mut *(*caller.data().vm).heap_ptr };
                            let ctx = crate::primitives::ctx::Alloc::new(heap);
                            let err = ctx.error("internal-error", format!("wasm: {}", e));
                            let (tag, payload) = caller.data_mut().inner.value_to_wasm(err);
                            return (tag, payload, 1);
                        }
                    }
                }

                // Bytecode closure: call back into the VM. Hand the callee its
                // executing-closure register via the one-shot (the WASM→interp
                // entry boundary), so a self-reference in the fallback body
                // resolves to the callee, not NIL.
                match vm_ref.build_closure_env(closure, &args) {
                    Some(env) => {
                        vm_ref.pending_entry_closure = func_val;
                        let exec =
                            vm_ref.execute_bytecode_saving_stack(&closure.template.code(), &env);
                        let bits = exec.bits;
                        if bits.is_empty() {
                            let (_, val) = vm_ref.fiber.signal.take().unwrap();
                            let (tag, payload) = caller.data_mut().inner.value_to_wasm(val);
                            (tag, payload, 0)
                        } else if bits == crate::value::SIG_HALT {
                            let val = vm_ref
                                .fiber
                                .signal
                                .as_ref()
                                .map(|(_, v)| *v)
                                .unwrap_or(Value::NIL);
                            if val == Value::NIL {
                                vm_ref.fiber.signal.take();
                                let (tag, payload) = caller.data_mut().inner.value_to_wasm(val);
                                (tag, payload, 0)
                            } else {
                                let (tag, payload) = caller.data_mut().inner.value_to_wasm(val);
                                (tag, payload, bits.raw() as i64)
                            }
                        } else {
                            let val = vm_ref
                                .fiber
                                .signal
                                .as_ref()
                                .map(|(_, v)| *v)
                                .unwrap_or(Value::NIL);
                            let (tag, payload) = caller.data_mut().inner.value_to_wasm(val);
                            (tag, payload, bits.raw() as i64)
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
                        (tag, payload, bits.raw() as i64)
                    }
                }
            } else {
                let heap = unsafe { &mut *(*caller.data().vm).heap_ptr };
                let ctx = crate::primitives::ctx::Alloc::new(heap);
                let err = ctx.error(
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
         -> (i64, i64, i64) {
            let args = read_args(&mut caller, args_ptr, nargs);
            let vm = caller.data().vm;
            let (bits, result) = super::super::linker::dispatch_data_op(op, &args, vm);
            let (tag, payload) = caller.data_mut().inner.value_to_wasm(result);
            (tag, payload, bits.raw() as i64)
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
         -> (i32, i32, i32, i64, i64, i64) {
            panic!("rt_prepare_tail_call called in tiered mode — should not happen");
        },
    )?;

    // rt_yield — stub (yield not yet supported in tiered/standalone mode)
    linker.func_wrap(
        "elle",
        "rt_yield",
        |_caller: Caller<'_, TieredHost>,
         _tag: i64,
         _payload: i64,
         _resume_state: i32,
         _regs_ptr: i32,
         _num_regs: i32,
         _func_idx: i32,
         _signal_bits: i64|
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
pub(super) fn read_args(
    caller: &mut Caller<'_, TieredHost>,
    args_ptr: i32,
    nargs: i32,
) -> Vec<Value> {
    let memory = caller
        .get_export("__elle_memory")
        .and_then(|e| e.into_memory())
        .expect("read_args: no memory");
    let data = memory.data(&*caller);
    super::super::handle::read_args_from_slice(
        data,
        &caller.data().inner.handles,
        args_ptr as usize,
        nargs.max(0) as usize,
    )
}
