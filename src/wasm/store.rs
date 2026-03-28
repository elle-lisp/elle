//! Wasmtime Engine/Store/Linker setup.

use wasmtime::*;

use super::host::ElleHost;
use crate::value::repr::TAG_HEAP_START;
use crate::value::Value;

/// Create a Wasmtime Engine with tail-call support.
pub fn create_engine() -> Result<Engine> {
    let mut config = Config::new();
    config.wasm_tail_call(true);
    config.wasm_multi_value(true);
    Engine::new(&config)
}

/// Create a Store with ElleHost state and pre-loaded constant pool.
pub fn create_store(engine: &Engine, const_pool: Vec<Value>) -> Store<ElleHost> {
    let mut host = ElleHost::new();

    // Pre-load heap constants into handle table.
    // The const_pool index maps 1:1 to the order rt_load_const will be called.
    for value in &const_pool {
        if value.tag >= TAG_HEAP_START {
            host.handles.insert(*value);
        }
    }

    host.const_pool = const_pool;
    Store::new(engine, host)
}

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
            let (bits, result) = caller.data().maybe_execute_io(bits, result);
            let (tag, payload) = caller.data_mut().value_to_wasm(result);
            (tag, payload, bits.0 as i32)
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
            if std::env::var_os("ELLE_WASM_DEBUG").is_some() {
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
                if std::env::var_os("ELLE_WASM_DEBUG").is_some() && nargs == 2 {
                    eprintln!(
                        "[rt_call] native 2args: [{}, {}]",
                        args[0].type_name(),
                        args[1].type_name()
                    );
                }
                let (bits, result) = native_fn(&args);
                if std::env::var_os("ELLE_WASM_DEBUG").is_some() && bits.0 != 0 {
                    eprintln!(
                        "[rt_call] native returned signal={} value={:?}",
                        bits.0, result
                    );
                }
                let (bits, result) = caller.data().maybe_execute_io(bits, result);

                // Handle SIG_RESUME: fiber/resume returns this signal.
                // Execute the fiber's WASM closure host-side.
                if bits.0 & 8 != 0 {
                    // SIG_RESUME: result is the fiber value
                    return handle_fiber_resume(&mut caller, result);
                }

                let (tag, payload) = caller.data_mut().value_to_wasm(result);
                (tag, payload, bits.0 as i32)
            } else if let Some((id, default)) = func_val.as_parameter() {
                if std::env::var_os("ELLE_WASM_DEBUG").is_some() {
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
                    // WASM closure: build env in linear memory and call
                    call_wasm_closure(&mut caller, closure, wasm_idx, &args)
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
                // Heap value — look up handle. Constants were pre-inserted
                // into the handle table in create_store, in order.
                // Handle index = index + 1 (handle 0 is reserved).
                let handle = (index + 1) as u64;
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

            // Create a ClosureTemplate with wasm_func_idx
            let template = std::rc::Rc::new(crate::value::closure::ClosureTemplate {
                bytecode: std::rc::Rc::new(vec![]),
                arity,
                num_locals,
                num_captures: num_captures as usize,
                num_params,
                constants: std::rc::Rc::new(vec![]),
                signal: crate::signals::Signal {
                    bits: crate::value::fiber::SignalBits(signal_bits),
                    propagates: 0,
                },
                lbox_params_mask,
                lbox_locals_mask,
                symbol_names: std::rc::Rc::new(std::collections::HashMap::new()),
                location_map: std::rc::Rc::new(crate::error::LocationMap::new()),
                jit_code: None,
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
                squelch_mask: 0,
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
            (tag, payload, bits.0 as i32)
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

            if std::env::var_os("ELLE_WASM_DEBUG").is_some() {
                eprintln!(
                    "[rt_prepare_tail_call] type={} nargs={}",
                    func_val.type_name(),
                    nargs
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
                    prepare_wasm_env(&mut caller, closure, &args, env_base);
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
                let (bits, result) = caller.data().maybe_execute_io(bits, result);
                let (tag, payload) = caller.data_mut().value_to_wasm(result);
                return (0, 0, 0, tag, payload, bits.0 as i32);
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

    // rt_yield(tag: i64, payload: i64, resume_state: i32, regs_ptr: i32, num_regs: i32)
    // Save yielded value and live registers to a WasmSuspensionFrame.
    linker.func_wrap(
        "elle",
        "rt_yield",
        |mut caller: Caller<'_, ElleHost>,
         tag: i64,
         payload: i64,
         resume_state: i32,
         regs_ptr: i32,
         num_regs: i32| {
            // Read saved registers from linear memory
            let saved_regs = read_reg_pairs(&mut caller, regs_ptr, num_regs);

            if std::env::var_os("ELLE_WASM_DEBUG").is_some() {
                eprintln!(
                    "[rt_yield] tag={} payload={} resume_state={} num_regs={}",
                    tag, payload, resume_state, num_regs
                );
            }

            let host = caller.data_mut();
            host.suspension_frames
                .push(super::host::WasmSuspensionFrame {
                    wasm_func_idx: 0, // filled by caller (call_wasm_closure)
                    resume_state: resume_state as u32,
                    saved_regs,
                    env_snapshot: Vec::new(), // filled by call_wasm_closure
                    env_base: 0,              // filled by call_wasm_closure
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
            match host.resume_value {
                Some((tag, payload)) => (tag, payload),
                None => {
                    // No resume value — return nil
                    (crate::value::repr::TAG_NIL as i64, 0)
                }
            }
        },
    )?;

    // rt_load_saved_reg(index: i32) -> (tag: i64, payload: i64)
    // Load a saved register by index from the current suspension frame.
    linker.func_wrap(
        "elle",
        "rt_load_saved_reg",
        |caller: Caller<'_, ElleHost>, index: i32| -> (i64, i64) {
            let host = caller.data();
            if let Some(frame) = host.suspension_frames.last() {
                if (index as usize) < frame.saved_regs.len() {
                    frame.saved_regs[index as usize]
                } else {
                    (crate::value::repr::TAG_NIL as i64, 0)
                }
            } else {
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

/// Build a WASM closure's environment in linear memory at `env_base`.
///
/// Layout: `[captures...][params...][local_slots...]`, each slot 16 bytes.
/// Handles varargs, LBox wrapping, and memory growth.
/// Updates `env_stack_ptr` to point past the new env region.
fn prepare_wasm_env(
    caller: &mut Caller<'_, ElleHost>,
    closure: &std::rc::Rc<crate::value::closure::Closure>,
    args: &[Value],
    env_base: usize,
) {
    let template = &closure.template;
    let num_captures = template.num_captures;
    let num_params = template.num_params;
    let num_locals = template.num_locals;
    let lbox_params_mask = template.lbox_params_mask;
    let lbox_locals_mask = template.lbox_locals_mask;

    // Handle varargs: if arity is AtLeast(n), collect extra args
    // into a list (or array) and pass as the last parameter.
    let effective_args;
    let args = match template.arity {
        crate::value::types::Arity::AtLeast(required) => {
            let mut collected = Vec::with_capacity(num_params);
            for arg in args.iter().take(required) {
                collected.push(*arg);
            }
            let rest: Vec<Value> = args[required..].to_vec();
            let vararg_val = match template.vararg_kind {
                crate::hir::VarargKind::List => {
                    let mut list = Value::EMPTY_LIST;
                    for v in rest.iter().rev() {
                        list = Value::cons(*v, list);
                    }
                    list
                }
                _ => Value::array_mut(rest),
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
    caller.data_mut().env_stack_ptr = env_base + total_slots * 16;

    let memory = caller
        .get_export("__elle_memory")
        .and_then(|e| e.into_memory())
        .expect("prepare_wasm_env: no memory");

    // Grow memory if needed
    let needed_bytes = env_base + total_slots * 16;
    let current_bytes = memory.data_size(&*caller);
    if needed_bytes > current_bytes {
        let pages_needed = (needed_bytes - current_bytes).div_ceil(65536) as u64;
        memory
            .grow(&mut *caller, pages_needed)
            .expect("prepare_wasm_env: failed to grow memory");
    }

    // Write captures from closure.env
    for (i, val) in closure.env.iter().enumerate() {
        let (tag, payload) = caller.data_mut().value_to_wasm(*val);
        let offset = env_base + i * 16;
        let data = memory.data_mut(&mut *caller);
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
        let (tag, payload) = caller.data_mut().value_to_wasm(val);
        let offset = env_base + (num_captures + i) * 16;
        let data = memory.data_mut(&mut *caller);
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
        let (tag, payload) = caller.data_mut().value_to_wasm(val);
        let offset = env_base + (num_captures + i) * 16;
        let data = memory.data_mut(&mut *caller);
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
        let (tag, payload) = caller.data_mut().value_to_wasm(val);
        let offset = env_base + (num_captures + num_params + i) * 16;
        let data = memory.data_mut(&mut *caller);
        data[offset..offset + 8].copy_from_slice(&tag.to_le_bytes());
        data[offset + 8..offset + 16].copy_from_slice(&payload.to_le_bytes());
    }
}

/// Call a WASM closure: build env in linear memory and invoke via table.
///
/// Each call allocates a fresh env region from `ElleHost::env_stack_ptr`
/// so that nested closure calls (recursion, higher-order) don't overwrite
/// each other's environments.
fn call_wasm_closure(
    caller: &mut Caller<'_, ElleHost>,
    closure: &std::rc::Rc<crate::value::closure::Closure>,
    wasm_idx: u32,
    args: &[Value],
) -> (i64, i64, i32) {
    let env_base = caller.data().env_stack_ptr;
    prepare_wasm_env(caller, closure, args, env_base);

    // Look up the WASM function in the table and call it
    let table = caller
        .get_export("__elle_table")
        .and_then(|e| e.into_table())
        .expect("call_wasm_closure: no table");
    let func_ref = table
        .get(&mut *caller, wasm_idx as u64)
        .expect("call_wasm_closure: table index out of bounds");
    let func = func_ref
        .unwrap_func()
        .expect("call_wasm_closure: table entry is not a function");

    let mut results = [Val::I64(0), Val::I64(0), Val::I32(0)];
    let call_result = func.call(
        &mut *caller,
        &[
            Val::I32(env_base as i32),
            Val::I32(0), // args_ptr unused
            Val::I32(0), // nargs unused
            Val::I32(0), // ctx
        ],
        &mut results,
    );

    match call_result {
        Ok(()) => {
            let tag = results[0].unwrap_i64();
            let payload = results[1].unwrap_i64();
            let status = results[2].unwrap_i32();

            if status > 0 {
                // Suspended: the WASM function yielded (or yield-through-call).
                // rt_yield already pushed a WasmSuspensionFrame with saved regs.
                // We need to snapshot the env and set metadata on the frame.

                // Snapshot env from linear memory
                let env_end = caller.data().env_stack_ptr;
                let env_size = env_end - env_base;
                let env_snapshot = if env_size > 0 {
                    let memory = caller
                        .get_export("__elle_memory")
                        .and_then(|e| e.into_memory())
                        .expect("call_wasm_closure: no memory");
                    let data = memory.data(&*caller);
                    data[env_base..env_end].to_vec()
                } else {
                    Vec::new()
                };

                // Update the suspension frame with env + metadata
                if let Some(frame) = caller.data_mut().suspension_frames.last_mut() {
                    frame.wasm_func_idx = wasm_idx;
                    frame.env_base = env_base;
                    frame.env_snapshot = env_snapshot;
                }

                // Restore env_stack_ptr (env is saved in snapshot)
                caller.data_mut().env_stack_ptr = env_base;

                if std::env::var_os("ELLE_WASM_DEBUG").is_some() {
                    eprintln!(
                        "[call_wasm_closure] SUSPENDED: status={} tag={} payload={}",
                        status, tag, payload
                    );
                }

                // Return the yielded value with SIG_YIELD
                (tag, payload, crate::value::fiber::SIG_YIELD.0 as i32)
            } else {
                // Normal return — restore env stack pointer
                caller.data_mut().env_stack_ptr = env_base;

                // Read signal from memory[0..4].
                let signal = {
                    let memory = caller
                        .get_export("__elle_memory")
                        .and_then(|e| e.into_memory())
                        .expect("call_wasm_closure: no memory");
                    let data = memory.data(&*caller);
                    i32::from_le_bytes(data[0..4].try_into().unwrap())
                };
                if signal != 0 {
                    let memory = caller
                        .get_export("__elle_memory")
                        .and_then(|e| e.into_memory())
                        .expect("call_wasm_closure: no memory");
                    memory.data_mut(&mut *caller)[0..4].copy_from_slice(&0i32.to_le_bytes());
                }

                if std::env::var_os("ELLE_WASM_DEBUG").is_some() {
                    let v = caller.data().wasm_to_value(tag, payload);
                    eprintln!(
                        "[call_wasm_closure] returned: tag={} payload={} signal={} status={} = {:?}",
                        tag, payload, signal, status, v
                    );
                }
                (tag, payload, signal)
            }
        }
        Err(e) => {
            // Restore env_stack_ptr on error too
            caller.data_mut().env_stack_ptr = env_base;
            let err = crate::value::error_val("exec-error", e.to_string());
            let (tag, payload) = caller.data_mut().value_to_wasm(err);
            (tag, payload, 1)
        }
    }
}

/// Resume a suspended WASM closure with a resume value.
///
/// Pops the outermost suspension frame, restores its env to linear memory,
/// sets the resume value, and re-invokes the WASM function with
/// `ctx = resume_state`. If the function suspends again, the new frame
/// is saved. If it returns normally, returns the result.
///
/// For multi-frame suspension chains (yield-through-call), the caller
/// must call this repeatedly: first resume the innermost callee, then
/// use its result to resume the next frame.
pub fn resume_wasm_closure(
    caller: &mut Caller<'_, ElleHost>,
    resume_val: Value,
) -> Option<(i64, i64, i32)> {
    // Don't pop the frame yet — rt_load_saved_reg needs to read from it
    // during the WASM function's resume prologue. Extract metadata first.
    let frame_idx = caller.data().suspension_frames.len().checked_sub(1)?;
    let wasm_func_idx = caller.data().suspension_frames[frame_idx].wasm_func_idx;
    let resume_state = caller.data().suspension_frames[frame_idx].resume_state;
    let env_base = caller.data().suspension_frames[frame_idx].env_base;
    let env_snapshot = caller.data().suspension_frames[frame_idx]
        .env_snapshot
        .clone();

    // Set resume value for rt_get_resume_value
    let (resume_tag, resume_pay) = caller.data_mut().value_to_wasm(resume_val);
    caller.data_mut().resume_value = Some((resume_tag, resume_pay));

    // Restore env to linear memory
    if !env_snapshot.is_empty() {
        let memory = caller
            .get_export("__elle_memory")
            .and_then(|e| e.into_memory())
            .expect("resume_wasm_closure: no memory");

        // Grow memory if needed
        let needed = env_base + env_snapshot.len();
        let current = memory.data_size(&*caller);
        if needed > current {
            let pages = (needed - current).div_ceil(65536) as u64;
            memory
                .grow(&mut *caller, pages)
                .expect("resume_wasm_closure: failed to grow memory");
        }

        let data = memory.data_mut(&mut *caller);
        data[env_base..env_base + env_snapshot.len()].copy_from_slice(&env_snapshot);
    }

    // Set env_stack_ptr past the restored env
    caller.data_mut().env_stack_ptr = env_base + env_snapshot.len();

    // Look up the WASM function in the table
    let table = caller
        .get_export("__elle_table")
        .and_then(|e| e.into_table())
        .expect("resume_wasm_closure: no table");
    let func_ref = table
        .get(&mut *caller, wasm_func_idx as u64)
        .expect("resume_wasm_closure: table index out of bounds");
    let func = func_ref
        .unwrap_func()
        .expect("resume_wasm_closure: table entry is not a function");

    // Call with ctx = resume_state
    let mut results = [Val::I64(0), Val::I64(0), Val::I32(0)];
    let call_result = func.call(
        &mut *caller,
        &[
            Val::I32(env_base as i32),
            Val::I32(0),                   // args_ptr unused
            Val::I32(0),                   // nargs unused
            Val::I32(resume_state as i32), // ctx = resume state
        ],
        &mut results,
    );

    // Clear resume value and pop the old suspension frame
    // (it was kept alive for rt_load_saved_reg during the prologue)
    caller.data_mut().resume_value = None;
    caller.data_mut().suspension_frames.remove(frame_idx);

    match call_result {
        Ok(()) => {
            let tag = results[0].unwrap_i64();
            let payload = results[1].unwrap_i64();
            let status = results[2].unwrap_i32();

            if status > 0 {
                // Suspended again — snapshot env and update frame (rt_yield pushed a new one)
                let env_end = caller.data().env_stack_ptr;
                let env_size = env_end - env_base;
                let env_snapshot = if env_size > 0 {
                    let memory = caller
                        .get_export("__elle_memory")
                        .and_then(|e| e.into_memory())
                        .expect("resume_wasm_closure: no memory");
                    memory.data(&*caller)[env_base..env_end].to_vec()
                } else {
                    Vec::new()
                };

                if let Some(new_frame) = caller.data_mut().suspension_frames.last_mut() {
                    new_frame.wasm_func_idx = wasm_func_idx;
                    new_frame.env_base = env_base;
                    new_frame.env_snapshot = env_snapshot;
                }

                caller.data_mut().env_stack_ptr = env_base;

                if std::env::var_os("ELLE_WASM_DEBUG").is_some() {
                    eprintln!("[resume_wasm_closure] SUSPENDED AGAIN: status={}", status);
                }
                Some((tag, payload, crate::value::fiber::SIG_YIELD.0 as i32))
            } else {
                // Normal return
                caller.data_mut().env_stack_ptr = env_base;

                let signal = {
                    let memory = caller
                        .get_export("__elle_memory")
                        .and_then(|e| e.into_memory())
                        .expect("resume_wasm_closure: no memory");
                    let data = memory.data(&*caller);
                    i32::from_le_bytes(data[0..4].try_into().unwrap())
                };
                if signal != 0 {
                    let memory = caller
                        .get_export("__elle_memory")
                        .and_then(|e| e.into_memory())
                        .expect("resume_wasm_closure: no memory");
                    memory.data_mut(&mut *caller)[0..4].copy_from_slice(&0i32.to_le_bytes());
                }

                if std::env::var_os("ELLE_WASM_DEBUG").is_some() {
                    let v = caller.data().wasm_to_value(tag, payload);
                    eprintln!(
                        "[resume_wasm_closure] returned: tag={} payload={} signal={} = {:?}",
                        tag, payload, signal, v
                    );
                }
                Some((tag, payload, signal))
            }
        }
        Err(e) => {
            caller.data_mut().env_stack_ptr = env_base;
            let err = crate::value::error_val("exec-error", e.to_string());
            let (tag, payload) = caller.data_mut().value_to_wasm(err);
            Some((tag, payload, 1))
        }
    }
}

/// Handle SIG_RESUME from fiber/resume in the WASM backend.
///
/// When `fiber/resume` returns SIG_RESUME, the fiber value contains the
/// fiber to execute. We extract it, run its WASM closure, update status.
fn handle_fiber_resume(caller: &mut Caller<'_, ElleHost>, fiber_value: Value) -> (i64, i64, i32) {
    use crate::value::fiber::{FiberStatus, SIG_ERROR, SIG_YIELD};

    let fiber_handle = match fiber_value.as_fiber() {
        Some(f) => f.clone(),
        None => {
            let err = crate::value::error_val("type-error", "fiber/resume: not a fiber");
            let (tag, payload) = caller.data_mut().value_to_wasm(err);
            return (tag, payload, SIG_ERROR.0 as i32);
        }
    };

    // Extract closure, resume value, and status from the fiber
    let (closure, resume_value, status) = fiber_handle.with_mut(|fiber| {
        let closure = fiber.closure.clone();
        let resume_value = fiber.signal.take().map(|(_, v)| v).unwrap_or(Value::NIL);
        let status = fiber.status;
        (closure, resume_value, status)
    });

    let wasm_idx = match closure.template.wasm_func_idx {
        Some(idx) => idx,
        None => {
            fiber_handle.with_mut(|f| f.status = FiberStatus::Error);
            let err =
                crate::value::error_val("internal-error", "fiber/resume: bytecode closure in WASM");
            let (tag, payload) = caller.data_mut().value_to_wasm(err);
            return (tag, payload, SIG_ERROR.0 as i32);
        }
    };

    let yield_signal = SIG_YIELD.0 as i32;

    match status {
        FiberStatus::New => {
            fiber_handle.with_mut(|f| f.status = FiberStatus::Alive);

            let args = if resume_value.is_nil() {
                vec![]
            } else {
                vec![resume_value]
            };
            let (tag, payload, signal) = call_wasm_closure(caller, &closure, wasm_idx, &args);

            if signal == yield_signal {
                let yielded = caller.data().wasm_to_value(tag, payload);
                fiber_handle.with_mut(|f| {
                    f.status = FiberStatus::Paused;
                    f.signal = Some((SIG_YIELD, yielded));
                });
                (tag, payload, 0) // caught by fiber
            } else if signal != 0 {
                fiber_handle.with_mut(|f| f.status = FiberStatus::Error);
                (tag, payload, signal)
            } else {
                fiber_handle.with_mut(|f| f.status = FiberStatus::Dead);
                (tag, payload, 0)
            }
        }
        FiberStatus::Paused => {
            fiber_handle.with_mut(|f| f.status = FiberStatus::Alive);

            let result = resume_wasm_closure(caller, resume_value);

            match result {
                Some((tag, payload, signal)) => {
                    if signal == yield_signal {
                        let yielded = caller.data().wasm_to_value(tag, payload);
                        fiber_handle.with_mut(|f| {
                            f.status = FiberStatus::Paused;
                            f.signal = Some((SIG_YIELD, yielded));
                        });
                        (tag, payload, 0)
                    } else if signal != 0 {
                        fiber_handle.with_mut(|f| f.status = FiberStatus::Error);
                        (tag, payload, signal)
                    } else {
                        // Resume chain for yield-through-call
                        let mut result_val = caller.data().wasm_to_value(tag, payload);
                        loop {
                            if caller.data().suspension_frames.is_empty() {
                                fiber_handle.with_mut(|f| f.status = FiberStatus::Dead);
                                let (t, p) = caller.data_mut().value_to_wasm(result_val);
                                return (t, p, 0);
                            }
                            match resume_wasm_closure(caller, result_val) {
                                Some((t, p, s)) => {
                                    if s == yield_signal {
                                        let yielded = caller.data().wasm_to_value(t, p);
                                        fiber_handle.with_mut(|f| {
                                            f.status = FiberStatus::Paused;
                                            f.signal = Some((SIG_YIELD, yielded));
                                        });
                                        return (t, p, 0);
                                    } else if s != 0 {
                                        fiber_handle.with_mut(|f| f.status = FiberStatus::Error);
                                        return (t, p, s);
                                    }
                                    result_val = caller.data().wasm_to_value(t, p);
                                }
                                None => {
                                    fiber_handle.with_mut(|f| f.status = FiberStatus::Dead);
                                    let (t, p) = caller.data_mut().value_to_wasm(result_val);
                                    return (t, p, 0);
                                }
                            }
                        }
                    }
                }
                None => {
                    fiber_handle.with_mut(|f| f.status = FiberStatus::Dead);
                    let (tag, payload) = caller.data_mut().value_to_wasm(Value::NIL);
                    (tag, payload, 0)
                }
            }
        }
        _ => {
            let err = crate::value::error_val("fiber-error", "fiber/resume: fiber not resumable");
            let (tag, payload) = caller.data_mut().value_to_wasm(err);
            (tag, payload, SIG_ERROR.0 as i32)
        }
    }
}

/// Dispatch a data operation by opcode.
fn dispatch_data_op(op: i32, args: &[Value]) -> (crate::value::fiber::SignalBits, Value) {
    use crate::value::fiber::{SIG_ERROR, SIG_OK};
    use crate::value::heap::TableKey;

    let err = |kind: &str, msg: &str| (SIG_ERROR, crate::value::error_val(kind, msg));

    match op {
        0 => (SIG_OK, Value::cons(args[0], args[1])), // OP_CONS
        1 => match args[0].as_cons() {
            // OP_CAR
            Some(c) => (SIG_OK, c.first),
            None => (SIG_OK, Value::NIL),
        },
        2 => match args[0].as_cons() {
            // OP_CDR
            Some(c) => (SIG_OK, c.rest),
            None => (SIG_OK, Value::NIL),
        },
        3 => match args[0].as_cons() {
            // OP_CAR_DESTRUCTURE
            Some(c) => (SIG_OK, c.first),
            None => err("type-error", "car: not a pair"),
        },
        4 => match args[0].as_cons() {
            // OP_CDR_DESTRUCTURE
            Some(c) => (SIG_OK, c.rest),
            None => err("type-error", "cdr: not a pair"),
        },
        5 => match args[0].as_cons() {
            // OP_CAR_OR_NIL
            Some(c) => (SIG_OK, c.first),
            None => (SIG_OK, Value::NIL),
        },
        6 => match args[0].as_cons() {
            // OP_CDR_OR_NIL
            Some(c) => (SIG_OK, c.rest),
            None => (SIG_OK, Value::EMPTY_LIST),
        },
        7 => (SIG_OK, Value::array_mut(args.to_vec())), // OP_MAKE_ARRAY
        8 => (SIG_OK, Value::local_lbox(args[0])),      // OP_MAKE_LBOX
        9 => {
            // OP_LOAD_LBOX
            match args[0].as_lbox() {
                Some(cell) => (SIG_OK, *cell.borrow()),
                None => (SIG_OK, args[0]),
            }
        }
        10 => {
            // OP_STORE_LBOX
            if let Some(cell) = args[0].as_lbox() {
                *cell.borrow_mut() = args[1];
            }
            (SIG_OK, Value::NIL)
        }
        11 => (SIG_OK, Value::NIL), // OP_MAKE_STRING (stub)
        12 => {
            // OP_ARRAY_REF_DESTRUCTURE
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
        13 => {
            // OP_ARRAY_SLICE_FROM
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
        14 => {
            // OP_STRUCT_GET_OR_NIL
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
        15 => {
            // OP_STRUCT_GET_DESTRUCTURE
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
        16 => {
            // OP_ARRAY_EXTEND
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
        17 => {
            // OP_ARRAY_PUSH
            if let Some(arr) = args[0].as_array_mut() {
                arr.borrow_mut().push(args[1]);
            }
            (SIG_OK, args[0])
        }
        18 => {
            // OP_ARRAY_LEN
            let len = if let Some(arr) = args[0].as_array_mut() {
                arr.borrow().len()
            } else if let Some(arr) = args[0].as_array() {
                arr.len()
            } else {
                0
            };
            (SIG_OK, Value::int(len as i64))
        }
        19 => {
            // OP_ARRAY_REF_OR_NIL
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
        20 => {
            // OP_STRUCT_REST: args[0] = struct, args[1..] = exclude keys
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

/// Read args from linear memory as Vec<Value>.
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

    // Guard against invalid nargs
    if !(0..=256).contains(&nargs) {
        panic!(
            "read_args_from_memory: invalid nargs={} args_ptr={}",
            nargs, args_ptr
        );
    }

    // First pass: read raw (tag, payload) pairs from memory
    let mut raw_pairs = Vec::with_capacity(nargs as usize);
    {
        let data = memory.data(&*caller);
        for i in 0..nargs as usize {
            let offset = args_ptr as usize + i * 16;
            let tag = i64::from_le_bytes(data[offset..offset + 8].try_into().unwrap()) as u64;
            let payload =
                i64::from_le_bytes(data[offset + 8..offset + 16].try_into().unwrap()) as u64;
            raw_pairs.push((tag, payload));
        }
    }

    // Second pass: resolve heap handles
    let host = caller.data();
    raw_pairs
        .into_iter()
        .map(|(tag, payload)| {
            if tag < TAG_HEAP_START {
                Value { tag, payload }
            } else {
                host.handles.get(payload)
            }
        })
        .collect()
}

/// Compile WASM bytes into a Module.
pub fn compile_module(engine: &Engine, wasm_bytes: &[u8]) -> Result<Module> {
    Module::new(engine, wasm_bytes)
}

/// Instantiate a module and call its entry function.
pub fn run_module(
    linker: &Linker<ElleHost>,
    store: &mut Store<ElleHost>,
    module: &Module,
) -> Result<Value> {
    let instance = linker.instantiate(&mut *store, module)?;
    let entry = instance.get_typed_func::<(), (i64, i64, i32)>(&mut *store, "__elle_entry")?;
    let (tag, payload, _status) = entry.call(&mut *store, ())?;
    let value = store.data().wasm_to_value(tag, payload);
    Ok(value)
}
