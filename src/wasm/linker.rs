//! Host function registration for the Wasmtime linker.

use wasmtime::*;

use super::host::ElleHost;
use crate::value::repr::TAG_HEAP_START;
use crate::value::Value;

/// Register host functions and return a Linker.
pub fn create_linker(engine: &Engine) -> Result<Linker<ElleHost>> {
    let mut linker = Linker::new(engine);

    // call_primitive(prim_id: i32, args_ptr: i32, nargs: i32, ctx: i32) -> (tag: i64, payload: i64, signal: i32)
    linker.func_wrap(
        "elle",
        "call_primitive",
        |mut caller: Caller<'_, ElleHost>,
         prim_id: i32,
         args_ptr: i32,
         nargs: i32,
         _ctx: i32|
         -> (i64, i64, i32) {
            let args = read_args_from_memory(&mut caller, args_ptr, nargs);
            let (bits, result) = caller.data_mut().call_primitive(prim_id as u32, &args);
            let (bits, result) = caller.data_mut().maybe_execute_io(bits, result);
            let (tag, payload) = caller.data_mut().value_to_wasm(result);
            (tag, payload, bits.raw() as i32)
        },
    )?;

    // rt_call(func_tag: i64, func_payload: i64, args_ptr: i32, nargs: i32, ctx: i32) -> (tag: i64, payload: i64, signal: i32)
    linker.func_wrap(
        "elle",
        "rt_call",
        |mut caller: Caller<'_, ElleHost>,
         func_tag: i64,
         func_payload: i64,
         args_ptr: i32,
         nargs: i32,
         _ctx: i32|
         -> (i64, i64, i32) {
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
                let (bits, result) = native_fn(&args);
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
                    let r = super::resume::handle_fiber_resume(&mut caller, result);
                    if caller.data().debug {
                        eprintln!(
                            "[rt_call] handle_fiber_resume returned: tag={} payload={} signal={}",
                            r.0, r.1, r.2
                        );
                    }
                    return r;
                }

                let (tag, payload) = caller.data_mut().value_to_wasm(result);
                (tag, payload, bits.raw() as i32)
            } else if let Some((id, default)) = func_val.as_parameter() {
                if caller.data().debug {
                    eprintln!("[rt_call] parameter id={} default={:?}", id, default);
                }
                if !args.is_empty() {
                    let err = crate::value::error_val(
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
                        super::store::call_precached_closure(&mut caller, closure, pc, &args)
                    } else {
                        // Fall back to full module's table
                        super::store::call_wasm_closure(&mut caller, closure, wasm_idx, &args)
                    }
                } else {
                    // Bytecode closure — not supported in WASM backend
                    let err = crate::value::error_val(
                        "internal-error",
                        "rt_call: bytecode closure in WASM backend",
                    );
                    let (tag, payload) = caller.data_mut().value_to_wasm(err);
                    (tag, payload, 1)
                }
            } else {
                let err = crate::value::error_val(
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
            let lbox_params_mask = read_i64(mp + 40) as u64;
            let lbox_locals_mask = read_i64(mp + 48) as u64;
            let signal_bits = read_i64(mp + 56) as u32;

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
                bytecode,
                arity,
                num_locals,
                num_captures: num_captures as usize,
                num_params,
                constants,
                signal: crate::signals::Signal {
                    bits: crate::value::fiber::SignalBits::new(signal_bits),
                    propagates: 0,
                },
                lbox_params_mask,
                lbox_locals_mask,
                symbol_names: std::rc::Rc::new(std::collections::HashMap::new()),
                location_map: std::rc::Rc::new(crate::error::LocationMap::new()),
                rotation_safe: false,
                lir_function: None,
                doc: None,
                syntax: None,
                vararg_kind: crate::hir::VarargKind::List,
                name: None,
                result_is_immediate: false,
                has_outward_heap_set: false,
                wasm_func_idx: Some(table_idx as u32),
            });

            let closure = crate::value::closure::Closure {
                template,
                env: std::rc::Rc::new(captures),
                squelch_mask: crate::value::fiber::SignalBits::EMPTY,
            };

            let value = Value::closure(closure);
            let (tag, payload) = caller.data_mut().value_to_wasm(value);
            (tag, payload)
        },
    )?;

    // rt_data_op(op: i32, args_ptr: i32, nargs: i32) -> (tag: i64, payload: i64, signal: i32)
    linker.func_wrap(
        "elle",
        "rt_data_op",
        |mut caller: Caller<'_, ElleHost>, op: i32, args_ptr: i32, nargs: i32| -> (i64, i64, i32) {
            let args = read_args_from_memory(&mut caller, args_ptr, nargs);
            let (bits, result) = dispatch_data_op(op, &args);
            let (tag, payload) = caller.data_mut().value_to_wasm(result);
            (tag, payload, bits.raw() as i32)
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
         -> (i32, i32, i32, i64, i64, i32) {
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
                    super::store::prepare_wasm_env(&mut caller, closure, &args, env_base);

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

                    return (env_base as i32, wasm_idx as i32, 1, 0, 0, 0);
                }
                let err = crate::value::error_val(
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
                let (bits, result) = native_fn(&args);
                let (bits, result) = caller.data_mut().maybe_execute_io(bits, result);
                // Write non-zero signal to memory[0..4] so handle_wasm_result
                // picks it up. The WASM tail call dispatch returns immediately
                // after this host call (just tag/payload/0), so no WASM code
                // overwrites memory[0..4] before the function exits.
                if bits.raw() != 0 {
                    if let Some(memory) = caller
                        .get_export("__elle_memory")
                        .and_then(|e| e.into_memory())
                    {
                        memory.data_mut(&mut caller)[0..4]
                            .copy_from_slice(&(bits.raw() as i32).to_le_bytes());
                    }
                }
                let (tag, payload) = caller.data_mut().value_to_wasm(result);
                return (0, 0, 0, tag, payload, bits.raw() as i32);
            }

            if let Some((id, default)) = func_val.as_parameter() {
                if !args.is_empty() {
                    let err = crate::value::error_val(
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

            let err = crate::value::error_val(
                "type-error",
                format!("rt_prepare_tail_call: cannot call {}", func_val.type_name()),
            );
            let (tag, payload) = caller.data_mut().value_to_wasm(err);
            (0, 0, 0, tag, payload, 1)
        },
    )?;

    // rt_yield(tag: i64, payload: i64, resume_state: i32, regs_ptr: i32, num_regs: i32, func_idx: i32, signal_bits: i32)
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
         signal_bits: i32| {
            // Read saved registers from linear memory
            let saved_regs = read_reg_pairs(&mut caller, regs_ptr, num_regs);

            if caller.data().debug {
                eprintln!(
                    "[rt_yield] tag={} payload={} resume_state={} num_regs={} func_idx={} signal_bits={}",
                    tag, payload, resume_state, num_regs, func_idx, signal_bits
                );
            }

            let host = caller.data_mut();
            host.push_suspension_frame(super::host::WasmSuspensionFrame {
                wasm_func_idx: func_idx as u32,
                resume_state: resume_state as u32,
                saved_regs,
                env_snapshot: Vec::new(),
                env_base: 0,
                signal_bits: signal_bits as u32,
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

/// Read (tag, payload) pairs from linear memory at `regs_ptr`.
fn read_reg_pairs(
    caller: &mut Caller<'_, ElleHost>,
    regs_ptr: i32,
    num_regs: i32,
) -> Vec<(i64, i64)> {
    if num_regs <= 0 {
        return Vec::new();
    }
    let memory = caller
        .get_export("__elle_memory")
        .and_then(|e| e.into_memory())
        .expect("read_reg_pairs: no memory");
    let data = memory.data(&*caller);
    let mut pairs = Vec::with_capacity(num_regs as usize);
    for i in 0..num_regs as usize {
        let offset = regs_ptr as usize + i * 16;
        let tag = i64::from_le_bytes(data[offset..offset + 8].try_into().unwrap());
        let payload = i64::from_le_bytes(data[offset + 8..offset + 16].try_into().unwrap());
        pairs.push((tag, payload));
    }
    pairs
}

/// Dispatch a data operation by opcode.
pub fn dispatch_data_op(op: i32, args: &[Value]) -> (crate::value::fiber::SignalBits, Value) {
    use super::emit::DataOp;
    use crate::value::fiber::{SIG_ERROR, SIG_OK};
    use crate::value::heap::TableKey;

    let err = |kind: &str, msg: &str| (SIG_ERROR, crate::value::error_val(kind, msg));

    match op {
        x if x == DataOp::Cons as i32 => (SIG_OK, Value::cons(args[0], args[1])),
        x if x == DataOp::Car as i32 => match args[0].as_cons() {
            Some(c) => (SIG_OK, c.first),
            None => (SIG_OK, Value::NIL),
        },
        x if x == DataOp::Cdr as i32 => match args[0].as_cons() {
            Some(c) => (SIG_OK, c.rest),
            None => (SIG_OK, Value::NIL),
        },
        x if x == DataOp::CarDestructure as i32 => match args[0].as_cons() {
            Some(c) => (SIG_OK, c.first),
            None => err("type-error", "car: not a pair"),
        },
        x if x == DataOp::CdrDestructure as i32 => match args[0].as_cons() {
            Some(c) => (SIG_OK, c.rest),
            None => err("type-error", "cdr: not a pair"),
        },
        x if x == DataOp::CarOrNil as i32 => match args[0].as_cons() {
            Some(c) => (SIG_OK, c.first),
            None => (SIG_OK, Value::NIL),
        },
        x if x == DataOp::CdrOrNil as i32 => match args[0].as_cons() {
            Some(c) => (SIG_OK, c.rest),
            None => (SIG_OK, Value::EMPTY_LIST),
        },
        x if x == DataOp::MakeArray as i32 => (SIG_OK, Value::array_mut(args.to_vec())),
        x if x == DataOp::MakeLBox as i32 => (SIG_OK, Value::local_lbox(args[0])),
        x if x == DataOp::LoadLBox as i32 => match args[0].as_lbox() {
            Some(cell) => (SIG_OK, *cell.borrow()),
            None => (SIG_OK, args[0]),
        },
        x if x == DataOp::StoreLBox as i32 => {
            if let Some(cell) = args[0].as_lbox() {
                *cell.borrow_mut() = args[1];
            }
            (SIG_OK, Value::NIL)
        }
        11 => (SIG_OK, Value::NIL), // MakeString (unused)
        x if x == DataOp::ArrayRefDestructure as i32 => {
            let index = args[1].payload as usize;
            if let Some(arr) = args[0].as_array_mut() {
                let b = arr.borrow();
                if index < b.len() {
                    (SIG_OK, b[index])
                } else {
                    err("index-error", "array ref: out of bounds")
                }
            } else if let Some(arr) = args[0].as_array() {
                if index < arr.len() {
                    (SIG_OK, arr[index])
                } else {
                    err("index-error", "array ref: out of bounds")
                }
            } else {
                err("type-error", "array ref: not an array")
            }
        }
        x if x == DataOp::ArraySliceFrom as i32 => {
            let index = args[1].payload as usize;
            if let Some(arr) = args[0].as_array_mut() {
                let b = arr.borrow();
                (SIG_OK, Value::array_mut(b[index.min(b.len())..].to_vec()))
            } else if let Some(arr) = args[0].as_array() {
                (
                    SIG_OK,
                    Value::array_mut(arr[index.min(arr.len())..].to_vec()),
                )
            } else {
                (SIG_OK, Value::array_mut(vec![]))
            }
        }
        x if x == DataOp::StructGetOrNil as i32 => {
            if let Some(s) = args[0].as_struct() {
                let key = match TableKey::from_value(&args[1]) {
                    Some(k) => k,
                    None => return (SIG_OK, Value::NIL),
                };
                (SIG_OK, s.get(&key).copied().unwrap_or(Value::NIL))
            } else if let Some(s) = args[0].as_struct_mut() {
                let key = match TableKey::from_value(&args[1]) {
                    Some(k) => k,
                    None => return (SIG_OK, Value::NIL),
                };
                (SIG_OK, s.borrow().get(&key).copied().unwrap_or(Value::NIL))
            } else {
                (SIG_OK, Value::NIL)
            }
        }
        x if x == DataOp::StructGetDestructure as i32 => {
            if let Some(s) = args[0].as_struct() {
                let key = match TableKey::from_value(&args[1]) {
                    Some(k) => k,
                    None => return (SIG_OK, Value::NIL),
                };
                match s.get(&key) {
                    Some(v) => (SIG_OK, *v),
                    None => err("key-error", "struct get: key not found"),
                }
            } else {
                err("type-error", "struct get: not a struct")
            }
        }
        x if x == DataOp::ArrayExtend as i32 => {
            if let Some(arr) = args[0].as_array_mut() {
                let source_elems: Vec<Value> = if let Some(src) = args[1].as_array_mut() {
                    src.borrow().to_vec()
                } else if let Some(src) = args[1].as_array() {
                    src.to_vec()
                } else if args[1].as_cons().is_some() || args[1].is_empty_list() {
                    match args[1].list_to_vec() {
                        Ok(v) => v,
                        Err(_) => {
                            return err("type-error", "splice: not a proper list");
                        }
                    }
                } else {
                    return err(
                        "type-error",
                        &format!(
                            "splice: expected array or list, got {}",
                            args[1].type_name()
                        ),
                    );
                };
                let mut vec = arr.borrow().to_vec();
                vec.extend(source_elems);
                (SIG_OK, Value::array_mut(vec))
            } else {
                (SIG_OK, args[0])
            }
        }
        x if x == DataOp::ArrayPush as i32 => {
            if let Some(arr) = args[0].as_array_mut() {
                arr.borrow_mut().push(args[1]);
            }
            (SIG_OK, args[0])
        }
        x if x == DataOp::ArrayLen as i32 => {
            let len = if let Some(arr) = args[0].as_array_mut() {
                arr.borrow().len()
            } else if let Some(arr) = args[0].as_array() {
                arr.len()
            } else {
                0
            };
            (SIG_OK, Value::int(len as i64))
        }
        x if x == DataOp::ArrayRefOrNil as i32 => {
            let index = args[1].payload as usize;
            if let Some(arr) = args[0].as_array_mut() {
                let b = arr.borrow();
                (SIG_OK, b.get(index).copied().unwrap_or(Value::NIL))
            } else if let Some(arr) = args[0].as_array() {
                (SIG_OK, arr.get(index).copied().unwrap_or(Value::NIL))
            } else {
                (SIG_OK, Value::NIL)
            }
        }
        x if x == DataOp::StructRest as i32 => {
            use std::collections::BTreeMap;
            let exclude_keys: Vec<TableKey> =
                args[1..].iter().filter_map(TableKey::from_value).collect();
            if let Some(s) = args[0].as_struct() {
                let filtered: BTreeMap<TableKey, Value> = s
                    .iter()
                    .filter(|(k, _)| !exclude_keys.contains(k))
                    .map(|(k, v)| (k.clone(), *v))
                    .collect();
                (SIG_OK, Value::struct_from(filtered))
            } else if let Some(s) = args[0].as_struct_mut() {
                let b = s.borrow();
                let filtered: BTreeMap<TableKey, Value> = b
                    .iter()
                    .filter(|(k, _)| !exclude_keys.contains(k))
                    .map(|(k, v)| (k.clone(), *v))
                    .collect();
                (SIG_OK, Value::struct_from(filtered))
            } else {
                (SIG_OK, Value::struct_from(BTreeMap::new()))
            }
        }
        _ => err("internal-error", &format!("rt_data_op: unknown op {op}")),
    }
}

/// Read args from linear memory as `Vec<Value>`.
fn read_args_from_memory(
    caller: &mut Caller<'_, ElleHost>,
    args_ptr: i32,
    nargs: i32,
) -> Vec<Value> {
    let memory = caller
        .get_export("__elle_memory")
        .and_then(|e| e.into_memory());
    let memory = match memory {
        Some(m) => m,
        None => return Vec::new(),
    };
    assert!(
        (0..=256).contains(&nargs),
        "read_args_from_memory: invalid nargs={} args_ptr={}",
        nargs,
        args_ptr
    );
    let data = memory.data(&*caller);
    super::handle::read_args_from_slice(
        data,
        &caller.data().handles,
        args_ptr as usize,
        nargs as usize,
    )
}
