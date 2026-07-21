use super::*;

/// Handle the result of a WASM closure call (shared by call and resume paths).
///
/// If the function suspended (status > 0): snapshot env to the front frame,
/// clear memory[0..8], restore env_stack_ptr, return SIG_YIELD.
/// If normal return: read signal from memory[0..8], clear if non-zero, return.
/// If error: restore env_stack_ptr, return error.
pub(in crate::wasm) fn handle_wasm_result(
    caller: &mut Caller<'_, ElleHost>,
    call_result: std::result::Result<(), wasmtime::Error>,
    results: &[Val; 3],
    env_base: usize,
    label: &str,
) -> (i64, i64, i64) {
    let memory = caller
        .get_export("__elle_memory")
        .and_then(|e| e.into_memory())
        .expect("handle_wasm_result: no memory");

    match call_result {
        Ok(()) => {
            let tag = results[0].unwrap_i64();
            let payload = results[1].unwrap_i64();
            let status = results[2].unwrap_i64();

            if status > 0 {
                // Suspended: snapshot env and update the back frame.
                let env_end = caller.data().env_stack_ptr;
                let env_snapshot = if env_end > env_base {
                    memory.data(&*caller)[env_base..env_end].to_vec()
                } else {
                    Vec::new()
                };

                if let Some(frame) = caller.data_mut().back_suspension_frame_mut() {
                    frame.env_base = env_base;
                    frame.env_snapshot = env_snapshot;
                }

                if caller.data().debug {
                    let old = i64::from_le_bytes(memory.data(&*caller)[0..8].try_into().unwrap());
                    eprintln!("[{}] clearing memory[0..8] from {} to 0", label, old);
                }
                memory.data_mut(&mut *caller)[0..8].copy_from_slice(&0i64.to_le_bytes());
                caller.data_mut().env_stack_ptr = env_base;

                if caller.data().debug {
                    eprintln!(
                        "[{}] SUSPENDED: status={} tag={} payload={}",
                        label, status, tag, payload
                    );
                }

                (tag, payload, crate::value::fiber::SIG_YIELD.raw() as i64)
            } else {
                caller.data_mut().env_stack_ptr = env_base;

                let mut signal =
                    i64::from_le_bytes(memory.data(&*caller)[0..8].try_into().unwrap());
                if signal != 0 {
                    memory.data_mut(&mut *caller)[0..8].copy_from_slice(&0i64.to_le_bytes());
                }
                // A NativeFn tail call that yielded SIG_IO (written to
                // memory[0..8] by rt_prepare_tail_call — the tail-position path a
                // stdlib wrapper like `tcp/connect`'s `(apply tcp/connect-ip …)`
                // takes) must ADD SIG_YIELD, not REPLACE with it: the WASM caller
                // keys yield-through off SIG_YIELD (bit 1), but the scheduler
                // keys IO submission off SIG_IO (bit 9) via fiber/bits. Dropping
                // SIG_IO here left the yielded io-request tagged as a plain yield,
                // so the scheduler re-queued the fiber and resumed it with nil
                // instead of submitting the IO — every tail-position IO
                // (`tcp/connect`, and the framing built on it) then read nil for
                // its port. Pinned by tests/elle/wasm-tail-io-in-fiber.lisp.
                if signal as u64 & crate::signals::SIG_IO.raw() != 0 {
                    signal |= crate::value::fiber::SIG_YIELD.raw() as i64;
                }

                if caller.data().debug {
                    let v = caller.data().wasm_to_value(tag, payload);
                    eprintln!(
                        "[{}] returned: tag={} payload={} signal={} = {:?}",
                        label, tag, payload, signal, v
                    );
                }
                (tag, payload, signal)
            }
        }
        Err(e) => {
            caller.data_mut().env_stack_ptr = env_base;
            let heap = unsafe { &mut *caller.data().heap_ptr() };
            let ctx = crate::primitives::ctx::Alloc::new(heap);
            let err = ctx.error("exec-error", e.to_string());
            let (tag, payload) = caller.data_mut().value_to_wasm(err);
            (tag, payload, 1)
        }
    }
}

/// Call a WASM closure: build env in linear memory and invoke via table.
///
/// Each call allocates a fresh env region from `ElleHost::env_stack_ptr`
/// so that nested closure calls (recursion, higher-order) don't overwrite
/// each other's environments.
pub(in crate::wasm) fn call_wasm_closure(
    caller: &mut Caller<'_, ElleHost>,
    closure: &crate::value::closure::Closure,
    wasm_idx: u32,
    args: &[Value],
    self_tag: i64,
    self_payload: i64,
) -> (i64, i64, i64) {
    let env_base = caller.data().env_stack_ptr;
    prepare_wasm_env(caller, closure, args, env_base);

    let table = caller
        .get_export("__elle_table")
        .and_then(|e| e.into_table())
        .expect("call_wasm_closure: no table");
    let table_size = table.size(&*caller);
    let func_ref = table.get(&mut *caller, wasm_idx as u64).unwrap_or_else(|| {
        panic!(
            "call_wasm_closure: table index {} out of bounds (table size {})",
            wasm_idx, table_size
        )
    });
    let func = func_ref
        .unwrap_func()
        .expect("call_wasm_closure: table entry is not a function");

    // Install this closure as the executing self, saving the caller's so it is
    // restored on return — the self slot is shared linear memory, so a nested call
    // must not leave the callee's self behind for the caller's `LoadSelf`. Restored
    // on EVERY exit path (normal, yield, error) before returning to the caller's
    // WASM, so a yielding callee's caller still sees its own self for yield-through.
    let memory = caller
        .get_export("__elle_memory")
        .and_then(|e| e.into_memory())
        .expect("call_wasm_closure: no memory");
    let saved_self = super::read_self_slot(&*caller, &memory);
    super::write_self_slot(&mut *caller, &memory, self_tag, self_payload);

    let mut results = [Val::I64(0), Val::I64(0), Val::I64(0)];
    let call_result = func.call(
        &mut *caller,
        &[
            Val::I32(env_base as i32),
            Val::I32(0),
            Val::I32(0),
            Val::I32(0),
        ],
        &mut results,
    );

    super::write_self_slot(&mut *caller, &memory, saved_self.0, saved_self.1);

    handle_wasm_result(caller, call_result, &results, env_base, "call_wasm_closure")
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
) -> Option<(i64, i64, i64)> {
    // Peek the front frame (innermost). During the WASM call, rt_load_saved_reg
    // reads from it. New frames pushed by rt_yield go to the back, so they
    // don't interfere. We pop_front AFTER the call completes.
    let frame = caller.data().first_suspension_frame()?;
    let wasm_func_idx = frame.wasm_func_idx;
    let resume_state = frame.resume_state;
    let env_snapshot = frame.env_snapshot.clone();
    let self_tag = frame.self_tag;
    let self_payload = frame.self_payload;

    // Restore the env at the CURRENT top of the shared env stack, not the
    // absolute base the frame occupied when it first suspended. The env stack is
    // a bump region shared by every fiber (and the top-level driver); a fresh
    // `call_wasm_closure` already bumps from this top. A resume must too: when an
    // interleaved driver resumes a parked fiber from DEEPER in its own call
    // stack than where that fiber suspended (the async scheduler resuming a
    // joined fiber inside `complete-fiber`), the stale base sits below the
    // driver's live env, so restoring there — and running the fiber upward from
    // it — overwrites the driver's own locals. The env is position-independent
    // (the function addresses its slots relative to the `env_base` it is passed),
    // so the snapshot restores identically at any base. Pinned by
    // tests/elle/wasm-collection-call.lisp's scheduler join and the
    // `wasm_full_scheduler_resumes_joined_fiber` unit test.
    let env_base = caller.data().env_stack_ptr;

    // Set resume value for rt_get_resume_value
    let (resume_tag, resume_pay) = caller.data_mut().value_to_wasm(resume_val);
    caller.data_mut().resume_value = Some((resume_tag, resume_pay));

    // Restore env to linear memory
    if !env_snapshot.is_empty() {
        let memory = caller
            .get_export("__elle_memory")
            .and_then(|e| e.into_memory())
            .expect("resume_wasm_closure: no memory");

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

    caller.data_mut().env_stack_ptr = env_base + env_snapshot.len();

    if caller.data().debug {
        eprintln!(
            "[resume_wasm_closure] env_base={} env_size={} resume_state={} wasm_func_idx={}",
            env_base,
            env_snapshot.len(),
            resume_state,
            wasm_func_idx
        );
        if !env_snapshot.is_empty() {
            let mut slots = Vec::new();
            let num_slots = env_snapshot.len() / 16;
            for i in 0..num_slots.min(4) {
                let off = i * 16;
                let tag = i64::from_le_bytes(env_snapshot[off..off + 8].try_into().unwrap());
                let pay = i64::from_le_bytes(env_snapshot[off + 8..off + 16].try_into().unwrap());
                slots.push(format!("({},{})", tag, pay));
            }
            eprintln!("[resume_wasm_closure] env slots: {:?}", slots);
        }
    }

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

    // Restore the executing closure this frame suspended with, so a `LoadSelf`
    // after resume names it — the store's shared self slot may hold another
    // closure's value from an interleaved fiber resumed in between.
    let self_memory = caller
        .get_export("__elle_memory")
        .and_then(|e| e.into_memory())
        .expect("resume_wasm_closure: no memory");
    super::write_self_slot(&mut *caller, &self_memory, self_tag, self_payload);

    let mut results = [Val::I64(0), Val::I64(0), Val::I64(0)];
    let call_result = func.call(
        &mut *caller,
        &[
            Val::I32(env_base as i32),
            Val::I32(0),
            Val::I32(0),
            Val::I32(resume_state as i32),
        ],
        &mut results,
    );

    // Pop the front frame now that the call is done. If the function yielded
    // again, rt_yield pushed new frames to the back — they survive this pop.
    caller.data_mut().pop_suspension_frame();
    caller.data_mut().resume_value = None;

    let (t, p, s) = handle_wasm_result(
        caller,
        call_result,
        &results,
        env_base,
        "resume_wasm_closure",
    );
    Some((t, p, s))
}

/// Compile WASM bytes into a Module.
pub fn compile_module(engine: &Engine, wasm_bytes: &[u8]) -> Result<Module> {
    Module::new(engine, wasm_bytes)
}

/// Instantiate a module and call its entry function.
/// If the entry function suspends (e.g. I/O inside ev/run), drive it
/// to completion by processing I/O inline via SyncBackend and resuming.
pub fn run_module(
    linker: &Linker<ElleHost>,
    store: &mut Store<ElleHost>,
    module: &Module,
) -> Result<Value> {
    use crate::io::request::IoRequest;
    use crate::signals::SIG_IO;

    let instance = linker.instantiate(&mut *store, module)?;
    let entry = instance.get_typed_func::<(i32,), (i64, i64, i64)>(&mut *store, "__elle_entry")?;
    let (mut tag, mut payload, mut status) = entry.call(&mut *store, (0,))?;

    // The entry function may suspend when ev/run's scheduler does I/O
    // (SIG_IO propagates through yield-through-call to the top level).
    // Drive it to completion by executing I/O inline and re-calling the
    // entry function with the resume state from its outermost frame.
    while status > 0 {
        let value = store.data().wasm_to_value(tag, payload);

        // Execute I/O if the innermost frame has SIG_IO
        let resume_val = if let Some(frame) = store.data().first_suspension_frame() {
            if frame.signal_bits & SIG_IO.raw() != 0 {
                if let Some(request) = value.as_external::<IoRequest>() {
                    let (_bits, result) = store.data_mut().execute_io_inline(request);
                    result
                } else {
                    value
                }
            } else {
                value
            }
        } else {
            break;
        };

        // Drain all suspension frames. The outermost (last) frame has the
        // entry function's resume_state; inner frames are discarded because
        // the entry function's CPS will re-create them on re-entry.
        let mut resume_state = 0i32;
        while store.data().has_suspension_frames() {
            if let Some(frame) = store.data_mut().pop_suspension_frame() {
                resume_state = frame.resume_state as i32;
            }
        }

        let (resume_tag, resume_pay) = store.data_mut().value_to_wasm(resume_val);
        store.data_mut().resume_value = Some((resume_tag, resume_pay));

        let (t, p, s) = entry.call(&mut *store, (resume_state,))?;
        store.data_mut().resume_value = None;
        tag = t;
        payload = p;
        status = s;
    }

    let value = store.data().wasm_to_value(tag, payload);

    // Surface an uncaught top-level error the way the VM does (a nonzero exit),
    // instead of returning the raised value as if it were a normal result. The
    // entry wraps the whole program in `ev/run`; when the program raises past
    // every handler — a bare `(error …)`, a failed `assert`, or `ev/run`'s
    // re-raise of an unjoined errored fiber — the entry unwinds leaving its
    // terminal signal word (linear `memory[0]`) NON-ZERO, and `value` holds the
    // error payload. A clean completion — including a CAUGHT error (`protect`) or
    // an error-shaped value returned WITHOUT raising — leaves `memory[0]` zero.
    // Without this the WASM tier is a weak oracle: every uncaught error is a
    // silent exit-0 false-pass, hiding real failures (and `assert`s) under
    // `--wasm=full`. Pinned by `wasm_full_uncaught_error_fails`.
    let terminal_signal = instance
        .get_memory(&mut *store, "__elle_memory")
        .map(|m| i64::from_le_bytes(m.data(&*store)[0..8].try_into().unwrap()))
        .unwrap_or(0);
    if terminal_signal != 0 {
        return Err(wasmtime::Error::msg(format!("Runtime error: {}", value)));
    }

    Ok(value)
}

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

    // Write params with optional LBox wrapping
    for (i, arg) in args.iter().enumerate().take(num_params) {
        let val = if i < 64 && capture_params_mask & (1u64 << i) != 0 {
            let region = fresh_region();
            crate::value::build::capture_cell(unsafe { &mut *env_heap_ptr }, *arg, region)
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
    pc: &super::super::host::PrecachedClosure,
    args: &[crate::value::Value],
    self_val: crate::value::Value,
) -> (i64, i64, i64) {
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
    let linker = match super::super::linker::create_linker(&engine) {
        Ok(l) => l,
        Err(e) => {
            let heap = unsafe { &mut *caller.data().heap_ptr() };
            let ctx = crate::primitives::ctx::Alloc::new(heap);
            let err = ctx.error("internal-error", e.to_string());
            let (tag, payload) = caller.data_mut().value_to_wasm(err);
            return (tag, payload, 1);
        }
    };
    let instance = match linker.instantiate(&mut store, &pc.module) {
        Ok(i) => i,
        Err(e) => {
            let heap = unsafe { &mut *caller.data().heap_ptr() };
            let ctx = crate::primitives::ctx::Alloc::new(heap);
            let err = ctx.error("internal-error", e.to_string());
            let (tag, payload) = caller.data_mut().value_to_wasm(err);
            return (tag, payload, 1);
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
    super::write_self_slot(&mut store, &memory, self_tag, self_payload);

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
            return (tag, payload, 1);
        }
    };

    match func.call(&mut store, (env_base as i32, 0, 0, 0)) {
        Ok((tag, payload, status)) => {
            // Convert result from standalone store's handle space
            // back to the caller's handle space.
            let value = store.data().wasm_to_value(tag, payload);
            let (caller_tag, caller_payload) = caller.data_mut().value_to_wasm(value);
            (caller_tag, caller_payload, status)
        }
        Err(e) => {
            let heap = unsafe { &mut *caller.data().heap_ptr() };
            let ctx = crate::primitives::ctx::Alloc::new(heap);
            let err = ctx.error("internal-error", e.to_string());
            let (tag, payload) = caller.data_mut().value_to_wasm(err);
            (tag, payload, 1)
        }
    }
}
