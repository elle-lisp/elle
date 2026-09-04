//! Array-call, closure-construction, tail-call, and env-building JIT entry points.

use super::*;

/// Unpacks the array and delegates to elle_jit_call.
///
/// `args_region` is the args array's own static slot: the array is the calling
/// convention's, so the call that consumes it reclaims it, on this tier exactly
/// as on the interpreter's (`VM::release_splice_args`,
/// docs/impl/region/mechanism.md § "A spliced call's arguments come out of an
/// array the convention owns").
#[no_mangle]
pub extern "C" fn elle_jit_call_array(
    func_tag: u64,
    func_payload: u64,
    args_array_tag: u64,
    args_array_payload: u64,
    vm: *mut (),
    region_id: u32,
    args_region: u32,
) -> JitValue {
    let args_val = Value {
        tag: args_array_tag,
        payload: args_array_payload,
    };
    let vm_ref = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let args_slot = crate::hir::region::StaticRegion::new(args_region)
        .expect("JIT region slot is nonzero — emitter invariant");
    // Claimed before the callee can park, exactly as on the interpreter tier.
    let args_array = vm_ref.take_splice_args(args_slot);

    let args: Vec<Value> = if let Some(arr) = args_val.as_array_mut() {
        arr.borrow().to_vec()
    } else if let Some(arr) = args_val.as_array() {
        arr.to_vec()
    } else {
        vm_ref.set_error(
            "type-error",
            format!(
                "splice: expected array or tuple for args, got {}",
                args_val.type_name()
            ),
        );
        vm_ref.release_splice_args(args_array);
        return JitValue::nil();
    };

    let nargs = args.len() as u32;
    let result = if args.is_empty() {
        elle_jit_call(
            func_tag,
            func_payload,
            std::ptr::null(),
            nargs,
            vm,
            region_id,
        )
    } else {
        elle_jit_call(func_tag, func_payload, args.as_ptr(), nargs, vm, region_id)
    };
    // The callee holds its own reference to every argument by now, so the
    // array's counted edges are surplus.
    unsafe { &mut *(vm as *mut crate::vm::VM) }.release_splice_args(args_array);
    result
}

/// Tail-call a function with arguments from an array value.
/// Unpacks the array and delegates to the shared tail-call body.
///
/// A spliced tail call MOVES nothing — its arguments came out of the args array,
/// not off this frame — so the callee mints one reference per parameter
/// (`own_params`), and the array's reclaim balances the pushes that built it.
#[no_mangle]
pub extern "C" fn elle_jit_tail_call_array(
    func_tag: u64,
    func_payload: u64,
    args_array_tag: u64,
    args_array_payload: u64,
    vm: *mut (),
    region_id: u32,
    args_region: u32,
) -> JitValue {
    let args_val = Value {
        tag: args_array_tag,
        payload: args_array_payload,
    };
    let vm_ref = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let args_slot = crate::hir::region::StaticRegion::new(args_region)
        .expect("JIT region slot is nonzero — emitter invariant");
    // Claimed before the callee can park, exactly as on the interpreter tier.
    let args_array = vm_ref.take_splice_args(args_slot);

    let args: Vec<Value> = if let Some(arr) = args_val.as_array_mut() {
        arr.borrow().to_vec()
    } else if let Some(arr) = args_val.as_array() {
        arr.to_vec()
    } else {
        vm_ref.set_error(
            "type-error",
            format!(
                "splice: expected array or tuple for args, got {}",
                args_val.type_name()
            ),
        );
        vm_ref.release_splice_args(args_array);
        return JitValue::nil();
    };

    let nargs = args.len() as u32;
    let result = if args.is_empty() {
        jit_tail_call_inner(
            func_tag,
            func_payload,
            std::ptr::null(),
            nargs,
            vm,
            region_id,
            true,
        )
    } else {
        jit_tail_call_inner(
            func_tag,
            func_payload,
            args.as_ptr(),
            nargs,
            vm,
            region_id,
            true,
        )
    };
    unsafe { &mut *(vm as *mut crate::vm::VM) }.release_splice_args(args_array);
    result
}

/// Create a closure from a code-object **blueprint** and captured environment.
/// `template_ptr`: raw pointer to a `TemplateProto` owned by the JIT code
/// object (`closure_protos`). `captures_ptr`: pointer to array of `count`
/// Values (16 bytes each). Materializes a FRESH region-allocated
/// `HeapObject::ClosureTemplate` header over the blueprint's shared payload,
/// into the current alloc region (set by the surrounding `push_alloc_region`
/// bracket), and builds the instance referencing it (co-region → region RC,
/// reclaimed when it frees).
#[no_mangle]
pub extern "C" fn elle_jit_make_closure(
    template_ptr: i64,
    captures_ptr: *const Value,
    count: u64,
    region: u32,
    vm: *mut (),
) -> JitValue {
    // The blueprint is owned by the JIT code object for as long as any code it
    // compiled can run, so this pointer is live. The header being built holds
    // its own counted handle rather than borrowing the code object's, so a
    // closure outliving its `JitCode` still reaches its blueprint.
    let blueprint = unsafe {
        let ptr = template_ptr as *const crate::value::TemplateProto;
        std::rc::Rc::increment_strong_count(ptr);
        std::rc::Rc::from_raw(ptr)
    };
    let count = count as usize;

    let env_slice: &[Value] = if count == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(captures_ptr, count) }
    };

    let region = crate::hir::region::RuntimeRegion::new(region)
        .expect("JIT alloc region id is a live mortal region");
    // The heap is the driving VM's own, reached through the threaded vm pointer —
    // this instance's heap, not a per-thread slot (docs/impl/region/ctx.md).
    let heap = unsafe { &mut *(*(vm as *mut crate::vm::VM)).heap_ptr };
    let result =
        crate::vm::closure::materialize_closure_in_region(heap, &blueprint, env_slice, region);
    JitValue::from_value(result)
}

// =============================================================================
// Internal Helpers
// =============================================================================

/// Convert an interpreter callee's full `ExecResult` (from
/// `execute_bytecode_saving_stack`) to a `JitValue`, parking its inner frame on
/// a non-yield suspend (SIG_FUEL and its compounds) so resume re-enters the
/// callee. `execute_bytecode_saving_stack` returns such a frame in
/// `ExecResult.stack` rather than in `fiber.suspended`; dropping it loses the
/// callee's state and resume injects nil as the call's return value
/// (`tests/elle/fuel-jit-preempt.lisp`). The suspend arm leaves the parked frame
/// in `fiber.suspended` and returns YIELD_SENTINEL; the compiled caller then
/// appends its own frame via `elle_jit_yield_through_call`. Every JIT site that
/// runs an interpreter callee through `execute_bytecode_saving_stack` — the
/// `elle_jit_call` fallback and the tail-call sentinel resolutions — routes its
/// result through here so the frame-preservation stays in one place.
pub(super) fn interp_exec_result_to_jit_value(
    vm: &mut crate::vm::VM,
    exec: crate::vm::execute::ExecResult,
) -> JitValue {
    let bits = exec.bits;
    if !bits.is_empty() && !bits.intersects(SIG_ERROR) && !bits.intersects(SIG_HALT) {
        let had_inner_stack = !exec.stack.is_empty();
        let mut frames = vm.fiber.suspended.take().unwrap_or_default();
        vm.park_suspended_callee_frame(&mut frames, bits, exec);
        debug_assert!(
            !had_inner_stack || !frames.is_empty(),
            "JIT→interpreter fallback dropped a fuel/signal-suspended callee's \
             inner frame; resume would inject nil for the call result \
             (tests/elle/fuel-jit-preempt.lisp)"
        );
        vm.fiber.suspended = Some(frames);
        return YIELD_SENTINEL;
    }
    exec_result_to_jit_value(vm, bits)
}

/// Convert an ExecResult's signal bits to a `JitValue`. Handles SIG_OK, SIG_HALT
/// (NIL→return value, else→error propagated via signal), suspending signals
/// (returns YIELD_SENTINEL), and errors. Callers holding the full `ExecResult`
/// of an interpreter callee use `interp_exec_result_to_jit_value` instead, which
/// also parks a fuel-suspended callee's inner frame.
pub(super) fn exec_result_to_jit_value(vm: &mut crate::vm::VM, bits: SignalBits) -> JitValue {
    if bits.is_empty() {
        let (_, val) = vm.fiber.signal.take().unwrap();
        JitValue::from_value(val)
    } else if bits == SIG_HALT {
        // (halt) → NIL → normal return. (halt <value>) → non-NIL → leave signal
        // in place for the JIT caller to detect via elle_jit_has_exception.
        let val = vm
            .fiber
            .signal
            .as_ref()
            .map(|(_, v)| *v)
            .unwrap_or(Value::NIL);
        if val == Value::NIL {
            vm.fiber.signal.take();
            JitValue::from_value(val)
        } else {
            // Non-NIL halt (stack overflow): signal stays set, JIT caller checks.
            JitValue::nil()
        }
    } else if bits.intersects(SIG_ERROR) {
        // SIG_ERROR — signal already set on fiber
        JitValue::nil()
    } else {
        // Any suspending signal (SIG_YIELD, SIG_SWITCH, user-defined) — side-exit
        YIELD_SENTINEL
    }
}

/// Handle a non-self tail call from JIT code.
///
/// If the target closure has JIT code in the cache, calls it directly.
/// Falls back to TAIL_CALL_SENTINEL (interpreter trampoline) only when
/// the target has no JIT code.
#[no_mangle]
pub extern "C" fn elle_jit_tail_call(
    func_tag: u64,
    func_payload: u64,
    args_ptr: *const Value,
    nargs: u32,
    vm: *mut (),
    region_id: u32,
) -> JitValue {
    jit_tail_call_inner(
        func_tag,
        func_payload,
        args_ptr,
        nargs,
        vm,
        region_id,
        false,
    )
}

/// The body of [`elle_jit_tail_call`], shared with the spliced entry point
/// [`elle_jit_tail_call_array`].
///
/// `spliced_args` is the same fact `tail_call_inner` reads on the interpreter
/// tier: an ordinary tail call MOVES its arguments (the caller's reference
/// transfers), while a spliced one holds them in an args array the convention
/// owns and reclaims, so the callee mints one reference per parameter instead.
#[allow(clippy::too_many_arguments)]
fn jit_tail_call_inner(
    func_tag: u64,
    func_payload: u64,
    args_ptr: *const Value,
    nargs: u32,
    vm: *mut (),
    region_id: u32,
    spliced_args: bool,
) -> JitValue {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let func = Value {
        tag: func_tag,
        payload: func_payload,
    };

    // Handle native functions — same region routing + pass-through retain as
    // the interpreter's `tail_call_inner` (see `VM::dispatch_native_call`).
    if let Some(def) = func.as_native_def() {
        let args_slice = args_ptr_to_value_slice(args_ptr, nargs);
        // Capability gate — identical to the interpreter's tail path
        // (`tail_call_inner`, src/vm/call/inner/tail.rs): a native whose signal
        // overlaps the fiber's withheld capabilities is denied, not run. Same gap
        // and fix as `elle_jit_call`'s Call-position path.
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

    // Handle parameter (dynamic binding lookup)
    if let Some((id, default)) = func.as_parameter() {
        if nargs != 0 {
            vm.set_error(
                "arity-error",
                format!("parameter call: expected 0 arguments, got {}", nargs),
            );
            return JitValue::nil();
        }
        let result = vm.resolve_parameter(id, default);
        // Pass-through retain, mirror of `tail_call_inner`'s parameter branch.
        // FIXME(leak): preserves the historical cross-id-space comparison; see
        // `VM::dispatch_native_call`. Revisited in the leak phase.
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

    // Handle closures — always use TAIL_CALL_SENTINEL so the trampoline
    // handles the call without growing the native stack.  This ensures
    // mutual tail recursion (A→B→A) doesn't overflow.
    if let Some(closure) = func.as_closure() {
        if !vm.check_arity(&closure.template.arity(), nargs as usize) {
            return JitValue::nil();
        }

        let args: Vec<Value> = (0..nargs as usize)
            .map(|i| unsafe { *args_ptr.add(i) })
            .collect();

        // Tail call: pure MOVE (own_params=false) — the caller's arg references
        // transfer to the callee, which releases them. A SPLICED tail call has
        // no such reference of the frame's: its arguments came out of the args
        // array, so the callee mints its own. Per-value env minting via
        // `populate_env`, same as the interpreter's `tail_call_inner`.
        let new_env = match vm.build_tail_call_env(closure, &args, spliced_args) {
            Some(env) => env,
            None => return JitValue::nil(), // bad keyword args — error on fiber
        };
        vm.pending_tail_call = Some(crate::vm::core::TailCallInfo {
            code: closure.template.code(),
            env: new_env,
            closure: func,
            squelch_mask: closure.squelch_mask,
        });
        // A deferral this spliced JIT tail call strands is not recorded on the
        // activation's dues: the callee-release and merged-arena channels are not
        // wired on this path, so the region stays held to the activation's own
        // teardown — a bounded over-keep, never an over-free — until they are
        // (docs/impl/region/owner.md § "A deferred tail-call release has the
        // node's life").

        return TAIL_CALL_SENTINEL;
    }

    // Callable collections: struct, array, set, string, bytes — in TAIL
    // position. Routed through the shared `dispatch_collection_call` for the
    // per-execution region + Rule-5 pass-through retain, then returned exactly
    // like the native-tail Ok path above (`jit_handle_primitive_signal` →
    // `JitValue::from_value`): the value is handed back in the return register
    // with its one owning reference, and the JIT-compiled caller's
    // post-`TailCall` block runs the owned-arg releases + result handling. The
    // old code set `fiber.signal = (SIG_OK, value)` AND skipped the retain, so a
    // tail-position call-index freed the returned co-located element under the
    // caller's borrow (JIT crash; the interpreter sibling leaked).
    if let Some(result) = {
        let region = crate::hir::region::StaticRegion::new(region_id)
            .expect("JIT region slot is nonzero — emitter invariant");
        vm.dispatch_collection_call(&func, args_ptr_to_value_slice(args_ptr, nargs), region)
    } {
        match result {
            Ok(value) => {
                return JitValue::from_value(value);
            }
            Err((kind, msg)) => {
                vm.set_error(kind, msg);
                return JitValue::nil();
            }
        }
    }

    vm.set_error("type-error", format!("Cannot call {}", vm.show_value(func)));
    JitValue::nil()
}

// =============================================================================
// Environment Building
// =============================================================================
//
// JIT closure-env construction is now unified on the interpreter's
// `VM::populate_env` (via `build_closure_env` / `build_tail_call_env` in
// `src/vm/env.rs`): the interpreter-fallback and tail paths call those directly
// so the owned-params `CallArgument` incref and per-value env-region minting are
// shared, not duplicated. The old `build_closure_env_for_jit` (which did neither,
// causing the owned-param over-release UAF and env-region commingling) is gone.
