// audited: 2026-11-13
//! The array-call JIT entry points, the conversion of an interpreter callee's result, and where the JIT builds an environment.
//!
//! docs/impl/jit.md
//! docs/impl/region/relocate.md

use super::tail::jit_tail_call_inner;
use super::*;

/// Unpacks the array and delegates to elle_jit_call.
///
/// `args_region` is the args array's own static slot: the array is the calling
/// convention's, so the call that consumes it reclaims it, on this tier exactly
/// as on the interpreter's (`VM::release_splice_args`,
/// docs/impl/region/mechanism.md).
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

    let args: Vec<Value> = if let Some(elems) = args_val.array_to_vec() {
        elems
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

    let args_ptr = if args.is_empty() {
        std::ptr::null()
    } else {
        args.as_ptr()
    };
    // A spliced call offers no move: the array holds the arguments' references.
    let result = elle_jit_call(
        func_tag,
        func_payload,
        args_ptr,
        args.len() as u32,
        vm,
        region_id,
        std::ptr::null_mut(),
    );
    // The callee holds its own reference to every argument by now, so the
    // array's counted edges are surplus.
    unsafe { &mut *(vm as *mut crate::vm::VM) }.release_splice_args(args_array);
    result
}

/// Tail-call a function with arguments from an array value.
/// Unpacks the array and delegates to the shared tail-call body.
///
/// A spliced tail call MOVES nothing: its arguments came out of the args array,
/// not off this frame. So the callee mints one reference per parameter
/// (`ArgRefs::tail`), and the array's reclaim balances the pushes that built it.
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

    let args: Vec<Value> = if let Some(elems) = args_val.array_to_vec() {
        elems
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

    let args_ptr = if args.is_empty() {
        std::ptr::null()
    } else {
        args.as_ptr()
    };
    // A spliced tail call carries neither deferral channel nor a stash on either
    // tier — `handle_tail_call_array` passes `false`/`None` for the same reason.
    let result = jit_tail_call_inner(
        func_tag,
        func_payload,
        args_ptr,
        args.len() as u32,
        vm,
        region_id,
        true,
        TailDeferrals::NONE,
        TailHandoff::NONE,
    );
    unsafe { &mut *(vm as *mut crate::vm::VM) }.release_splice_args(args_array);
    result
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
/// (`tests/impl/fuel-jit-preempt.lisp`). The suspend arm leaves the parked frame
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
             (tests/impl/fuel-jit-preempt.lisp)"
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
        // in place for the JIT caller to detect via elle_jit_has_signal.
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

// =============================================================================
// Environment Building
// =============================================================================
//
// JIT closure-env construction is the interpreter's `VM::populate_env`, reached
// through `build_call_env` / `pending_tail_call_to` (`src/vm/env.rs`). The
// interpreter-fallback and tail paths call those directly, so the owned-params
// `CallArgument` incref and the per-value env-region minting are one
// implementation. A second construction here would owe both, and an env built
// without them over-releases every owned param and commingles the env cells of
// two calls through one slot.
