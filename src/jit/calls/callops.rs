use super::*;

/// Call a function from JIT code.
///
/// Dispatches to native functions or closures. When the callee has
/// JIT-compiled code in the cache, calls it directly (JIT-to-JIT)
/// without building an interpreter environment — zero heap allocations
/// on the fast path.
///
/// Parameters: func_tag/func_payload (the callee Value), args_ptr (*const Value),
/// nargs, vm.
/// Returns a `JitValue` for the result.
#[no_mangle]
pub extern "C" fn elle_jit_call(
    func_tag: u64,
    func_payload: u64,
    args_ptr: *const Value,
    nargs: u32,
    vm: *mut (),
    region_id: u32,
) -> JitValue {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let func = Value {
        tag: func_tag,
        payload: func_payload,
    };

    // Dispatch to native function — zero-copy args via *const Value.
    // `dispatch_native_call` routes the result allocation into this call's
    // region and applies the pass-through retain, identical to the
    // interpreter's `call_inner`; skipping it would under-count a pass-through
    // result's region and free it under a freshly built cons (UAF).
    if let Some(def) = func.as_native_def() {
        let args_slice = args_ptr_to_value_slice(args_ptr, nargs);
        // Capability gate — identical to the interpreter's `call_inner`
        // (src/vm/call/inner.rs): a native whose signal bits overlap the fiber's
        // withheld capabilities is denied, not run. Without this the JIT would run
        // a withheld primitive and suspend on its raw effect request instead of the
        // denial payload (pinned by region-capability-denial-value.lisp under
        // `--jit`).
        let blocked = def
            .signal
            .bits
            .intersection(vm.fiber.withheld)
            .intersection(crate::signals::CAP_MASK);
        if !blocked.is_empty() {
            return crate::jit::calls::jit_capability_denial(vm, def, blocked, args_slice);
        }
        // The JIT passes the call's static region slot across the C ABI as a
        // bare `u32`; rewrap it into its `StaticRegion` newtype at the boundary.
        // The emitter only ever bakes a nonzero slot (no in-band 0), so `expect`
        // documents that invariant.
        let region = crate::hir::region::StaticRegion::new(region_id)
            .expect("JIT region slot is nonzero — emitter invariant");
        let (bits, value) = vm.dispatch_native_call(def, args_slice, region);
        return jit_handle_primitive_signal(vm, bits, value);
    }

    // Dispatch to parameter (dynamic binding lookup)
    if let Some((id, default)) = func.as_parameter() {
        if nargs != 0 {
            vm.set_error(
                "arity-error",
                format!("parameter call: expected 0 arguments, got {}", nargs),
            );
            return JitValue::nil();
        }
        let result = vm.resolve_parameter(id, default);
        // Pass-through retain, mirror of `call_inner`'s parameter branch: a
        // resolve returns a value bound in the dynamic-binding frame (region ≠
        // this call's region_id), so hand the caller one owning reference for
        // its `DecrefValueRegion` to consume.
        // FIXME(leak): preserves the historical cross-id-space comparison
        // (runtime `result_region` vs static `region_id`); see the matching
        // note in `VM::dispatch_native_call`. Revisited in the leak phase.
        // A parameter resolve never allocates a fresh region — always hand the
        // caller one owning reference for its `DecrefValueRegion` to consume.
        // `incref_for_escape(None, …)` no-ops an immediate. (Was gated on a
        // static-vs-runtime `r.get() == region_id` compare — see the leak note
        // in `VM::dispatch_native_call`.)
        let heap = unsafe { &mut *vm.heap_ptr };
        let result_region = crate::value::arena::region_of(heap, result);
        crate::value::arena::incref_for_escape(
            heap,
            result_region,
            crate::value::arena::EscapeSite::ParameterResolve,
        );
        return JitValue::from_value(result);
    }

    // Dispatch to closure
    if let Some(closure) = func.as_closure() {
        if !vm.check_arity(&closure.template.arity, nargs as usize) {
            return JitValue::nil();
        }

        let closure_squelch_mask = closure.squelch_mask;

        // JIT-to-JIT fast path: check if callee has JIT code
        let bytecode_ptr = closure.template.bytecode.as_ptr();
        if let Some(jit_code) = vm.jit_cache.get(&bytecode_ptr).cloned() {
            vm.fiber.call_depth += 1;

            // Stack overflow guard: resource exhaustion (not signal-theoretic).
            // Uses SIG_HALT so the condition bypasses all signal masks.
            if vm.fiber.call_depth > MAX_CALL_DEPTH {
                vm.fiber.call_depth -= 1;
                let err = vm.escaping_error(
                    "stack-overflow",
                    format!("call depth exceeded maximum ({})", MAX_CALL_DEPTH),
                );
                vm.fiber.signal = Some((SIG_HALT, err));
                return JitValue::nil();
            }

            let env_ptr = if closure.env.is_empty() {
                std::ptr::null()
            } else {
                closure.env.as_ptr()
            };

            // Non-tail call: the JIT callee owns each non-captured fixed param
            // and releases it value-based (`DecrefValueRegion`) at its
            // decref_point. The fast path hands args by pointer with no env
            // build, so hand over one `CallArgument` owning reference per such
            // param here — the JIT-to-JIT mirror of `populate_env`'s own_params
            // incref. Without it the callee over-releases a heap arg (UAF).
            incref_owned_call_args(
                unsafe { &mut *vm.heap_ptr },
                closure,
                args_ptr_to_value_slice(args_ptr, nargs),
            );

            let result = unsafe {
                jit_code.call(
                    env_ptr,
                    args_ptr,
                    nargs,
                    vm as *mut crate::vm::VM as *mut (),
                    func_tag,
                    func_payload,
                )
            };

            vm.fiber.call_depth -= 1;

            // Check for exception (error or halt) — use contains for compound signals
            if vm
                .fiber
                .signal
                .as_ref()
                .is_some_and(|(b, _)| b.intersects(SIG_ERROR) || b.intersects(SIG_HALT))
            {
                return JitValue::nil();
            }

            // Check for suspending signal from callee (SIG_YIELD, SIG_SWITCH, user-defined)
            if let Some((sig, _)) = vm.fiber.signal {
                if !sig.is_empty() && !sig.intersects(SIG_ERROR) && !sig.intersects(SIG_HALT) {
                    // Squelch enforcement on the JIT-to-JIT path
                    if !closure_squelch_mask.is_empty() {
                        let squelched = sig.intersection(closure_squelch_mask);
                        if !squelched.is_empty() {
                            let squelched_str = {
                                let registry =
                                    crate::signals::registry::global_registry().lock().unwrap();
                                registry.format_signal_bits(squelched)
                            };
                            let err = vm.escaping_error(
                                "signal-violation",
                                format!("squelch: signal {} caught at boundary", squelched_str),
                            );
                            // The squelch discard chokepoint — frees each parked
                            // frame's owner node, exactly as `enforce_squelch`
                            // does on the interpreter path.
                            vm.discard_suspended_frames();
                            vm.fiber.signal = Some((SIG_ERROR, err));
                            return JitValue::nil();
                        }
                    }
                    return YIELD_SENTINEL;
                }
            }

            // Handle tail call sentinel. The resolved body is the tail callee's:
            // hand it its executing-closure register via the one-shot, exactly as
            // the interpreter's `trampoline_loop` installs `tail.closure` on a
            // frame replacement — a self-reference in that body resolves to it.
            if result == TAIL_CALL_SENTINEL {
                if let Some(tail) = vm.pending_tail_call.take() {
                    vm.pending_entry_closure = tail.closure;
                    let exec_result = vm.execute_bytecode_saving_stack(&tail.code, &tail.env);
                    // Park the tail callee's inner frame on a fuel/signal suspend
                    // so resume re-enters it (interp_exec_result_to_jit_value); a
                    // tail-recursive interpreter callee (e.g. `fold`) otherwise
                    // loses its accumulator across preemption.
                    return interp_exec_result_to_jit_value(vm, exec_result);
                }
            }

            // Defensive: if callee returned YIELD_SENTINEL without setting signal
            if result == YIELD_SENTINEL {
                return YIELD_SENTINEL;
            }

            return result;
        }

        // Interpreter fallback — reconstruct args Vec for env building
        let args: Vec<Value> = (0..nargs as usize)
            .map(|i| unsafe { *args_ptr.add(i) })
            .collect();

        let closure_squelch_mask = closure.squelch_mask;
        // Non-tail call: the callee owns each non-captured fixed param and
        // releases it value-based at its `decref_point`. `build_closure_env`
        // (own_params=true) hands the callee one `CallArgument` owning reference
        // per such arg and mints each env cell/cons in its own region — the same
        // path as the interpreter's `call_inner`. Without this the callee's
        // `DecrefValueRegion` over-releases a heap arg (UAF).
        let new_env = match vm.build_closure_env(closure, &args) {
            Some(env) => env,
            None => return JitValue::nil(), // bad keyword args — error on fiber
        };

        vm.fiber.call_depth += 1;

        // Stack overflow guard: resource exhaustion (not signal-theoretic).
        // Uses SIG_HALT so the condition bypasses all signal masks.
        if vm.fiber.call_depth > MAX_CALL_DEPTH {
            vm.fiber.call_depth -= 1;
            let err = vm.escaping_error(
                "stack-overflow",
                format!("call depth exceeded maximum ({})", MAX_CALL_DEPTH),
            );
            vm.fiber.signal = Some((SIG_HALT, err));
            return JitValue::nil();
        }

        // Hand the callee its executing-closure register via the one-shot, the
        // same handoff the interpreter's `call_inner` performs — a self-reference
        // in the fallback body resolves to `func`, not `NIL`.
        vm.pending_entry_closure = func;
        let result = vm.execute_bytecode_saving_stack(&closure.template.code(), &new_env);
        vm.fiber.call_depth -= 1;

        // Squelch enforcement: if the closure has a squelch mask and the callee
        // returned a suspending signal that matches, convert to signal-violation.
        let bits = result.bits;
        if !closure_squelch_mask.is_empty()
            && !bits.is_empty()
            && !bits.intersects(SIG_ERROR)
            && !bits.intersects(SIG_HALT)
        {
            let squelched = bits.intersection(closure_squelch_mask);
            if !squelched.is_empty() {
                let squelched_str = {
                    let registry = crate::signals::registry::global_registry().lock().unwrap();
                    registry.format_signal_bits(squelched)
                };
                let err = vm.escaping_error(
                    "signal-violation",
                    format!("squelch: signal {} caught at boundary", squelched_str),
                );
                // The squelch discard chokepoint (see the JIT-to-JIT arm above).
                vm.discard_suspended_frames();
                vm.fiber.signal = Some((SIG_ERROR, err));
                return JitValue::nil();
            }
        }

        // Route through the shared converter, which parks the callee's inner
        // frame on a non-yield suspend (SIG_FUEL) so resume re-enters it — see
        // interp_exec_result_to_jit_value. `bits` above already drove the squelch
        // check; the converter re-reads it from `result`.
        interp_exec_result_to_jit_value(vm, result)
    } else if let Some(result) = {
        // Collection-as-function call-index, routed through the shared
        // `dispatch_collection_call` so the JIT applies the same per-execution
        // region + Rule-5 pass-through retain as the interpreter; otherwise a
        // co-located/stored element is freed under the caller's `DecrefValueRegion`
        // (the call-index UAF family, RED on the JIT tier too).
        let region = crate::hir::region::StaticRegion::new(region_id)
            .expect("JIT region slot is nonzero — emitter invariant");
        vm.dispatch_collection_call(&func, args_ptr_to_value_slice(args_ptr, nargs), region)
    } {
        match result {
            Ok(value) => JitValue::from_value(value),
            Err((kind, msg)) => {
                vm.set_error(kind, msg);
                JitValue::nil()
            }
        }
    } else {
        vm.set_error("type-error", format!("Cannot call {:?}", func));
        JitValue::nil()
    }
}

/// Resolve a pending tail call after a direct SCC call.
///
/// When a directly-called SCC peer returns TAIL_CALL_SENTINEL (because it
/// tail-called something outside the SCC), the caller must resolve it.
/// This helper checks for the sentinel and executes the pending tail call.
///
/// Returns the final `JitValue`, or `JitValue::nil()` if an error occurred.
#[no_mangle]
pub extern "C" fn elle_jit_resolve_tail_call(
    result_tag: u64,
    result_payload: u64,
    vm: *mut (),
) -> JitValue {
    let result = JitValue {
        tag: result_tag,
        payload: result_payload,
    };
    if result != TAIL_CALL_SENTINEL {
        return result;
    }
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    if let Some(tail) = vm.pending_tail_call.take() {
        // The resolved body is the tail callee's — hand it its
        // executing-closure register (see `elle_jit_call`'s sentinel arm).
        vm.pending_entry_closure = tail.closure;
        let exec_result = vm.execute_bytecode_saving_stack(&tail.code, &tail.env);
        // Park the tail callee's inner frame on a fuel/signal suspend (see the
        // sentinel arm in elle_jit_call).
        interp_exec_result_to_jit_value(vm, exec_result)
    } else {
        panic!(
            "VM bug: TAIL_CALL_SENTINEL returned but no pending_tail_call set. \
             This indicates a bug in the JIT tail call protocol."
        );
    }
}

/// Release heap objects at a self-tail-call boundary in JIT code.
///
/// Called from the JIT self-tail-call loop after reading argument values
/// JIT pool rotation — now a no-op (regions handle deallocation via FreeRegion).
#[no_mangle]
pub extern "C" fn elle_jit_rotate_pools(_vm: *mut ()) {}

/// Increment call depth and check for stack overflow.
///
/// Returns FALSE on success, or TRUE if the call depth exceeds 1000
/// (after setting the error signal on the fiber).
#[no_mangle]
pub extern "C" fn elle_jit_call_depth_enter(vm: *mut ()) -> JitValue {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    vm.fiber.call_depth += 1;
    JitValue::bool_val(false) // falsy — ok
}

/// Decrement call depth after a direct SCC call returns.
///
/// Pairs with `elle_jit_call_depth_enter`. Always returns NIL (ignored).
#[no_mangle]
pub extern "C" fn elle_jit_call_depth_exit(vm: *mut ()) -> JitValue {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    vm.fiber.call_depth -= 1;
    JitValue::nil()
}

/// Pop one dynamic parameter frame from the fiber.
/// Pairs with PushParamFrame. Returns NIL (ignored by caller).
#[no_mangle]
pub extern "C" fn elle_jit_pop_param_frame(vm: *mut ()) -> JitValue {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    vm.fiber.param_frames.pop();
    JitValue::nil()
}

/// Call a function with arguments from an array value.
mod arraycall;
pub use arraycall::*;
