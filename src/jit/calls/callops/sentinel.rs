// audited: 2026-11-24
//! The tail call a compiled callee leaves pending when it returns the
//! tail-call sentinel, run for `elle_jit_call` and for a direct self-call.
//!
//! docs/impl/jit.md
//! docs/impl/jit-selfcall.md

use super::*;
use crate::value::closure::BoundaryParts;
use crate::value::fiber::ParamDepth;

/// Run the tail call pending on `vm`, or answer `None` when none is pending.
///
/// The activation opens on the tail callee, with the callee as its
/// executing-closure register, as the interpreter's trampoline installs it on
/// a frame replacement. Its signal crosses its own boundary and that of the
/// callee whose code returned the sentinel, `parts`, and the callee's proof
/// comes first. `depth` is the parameter depth at the call.
pub(super) fn run_pending_tail(
    vm: &mut crate::vm::VM,
    parts: &BoundaryParts,
    depth: ParamDepth,
) -> Option<JitValue> {
    let mut tail = vm.pending_tail_call.take()?;
    let mut boundary = parts.boundary();
    boundary.add(tail.take_boundary());
    let exec_result = vm.execute_tail_saving_stack(tail);
    if vm
        .enforce_boundary(exec_result.bits, &boundary, depth)
        .is_some()
    {
        return Some(JitValue::nil());
    }
    // Park the tail callee's inner frame on a fuel/signal suspend so resume
    // re-enters it (interp_exec_result_to_jit_value); a tail-recursive
    // interpreter callee (for example `fold`) otherwise loses its accumulator
    // across preemption.
    Some(interp_exec_result_to_jit_value(vm, exec_result))
}

/// Run the tail call that a direct self-call's callee left pending with the
/// sentinel, inside the boundary of the running closure, `self_tag` and
/// `self_payload`.
///
/// The running activation holds the closure, so its parts are live here. The
/// parameter depth now is the depth at the call, because a `parameterize`
/// body is never in tail position. With no tail call pending, answers the
/// sentinel, as `elle_jit_call` does.
#[no_mangle]
pub extern "C" fn elle_jit_self_call_tail(
    self_tag: u64,
    self_payload: u64,
    vm: *mut (),
) -> JitValue {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let func = Value {
        tag: self_tag,
        payload: self_payload,
    };
    let Some(closure) = func.as_closure() else {
        vm.set_error("type-error", format!("Cannot call {}", vm.show_value(func)));
        return JitValue::nil();
    };
    let parts = BoundaryParts::of(closure);
    let depth = vm.fiber.param_depth();
    run_pending_tail(vm, &parts, depth).unwrap_or(TAIL_CALL_SENTINEL)
}
