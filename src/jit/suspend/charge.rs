// audited: 2026-10-22
//! The two exits at a compiled charge site: the fuel pause, which parks the frame, and the kill, which parks nothing.
//!
//! docs/impl/jit-fuel.md

use super::{park_activation, rebuild_frame, ParkSite, SpillCounts, YIELD_SENTINEL};
use crate::jit::value::JitValue;
use crate::value::Value;

/// JIT fuel pause: park this frame at a charge site whose budget ran out.
///
/// Called from JIT code when the inline charge finds the fiber's budget at
/// zero. Builds the frame at the charging instruction's bytecode offset, with
/// no resume value, so the interpreter runs that instruction again. The frame
/// starts the suspended chain, as the frame of a yield does, and each compiled
/// caller parks behind it on the way out.
///
/// Parameters:
///   spilled_values: *const Value (16 bytes each)
///   charge_site_index: index into JitCode.charge_sites
///   vm: *mut () as u64
///   closure_tag/closure_payload: the closure being executed
///
/// Returns YIELD_SENTINEL.
///
/// # Safety
/// `spilled_values` must point to `num_spilled` contiguous `Value`s
/// (or be null when num_spilled is 0).
#[no_mangle]
pub extern "C" fn elle_jit_fuel_pause(
    spilled_values: *const Value,
    charge_site_index: u64,
    vm: u64, // *mut () as u64
    closure_tag: u64,
    closure_payload: u64,
) -> JitValue {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let closure_val = Value {
        tag: closure_tag,
        payload: closure_payload,
    };

    let closure = closure_val
        .as_closure()
        .expect("VM bug: elle_jit_fuel_pause called with non-closure");

    let bytecode_ptr = closure.template.bytecode().as_ptr();
    let jit_code = vm
        .jit_code_for(bytecode_ptr)
        .expect("VM bug: elle_jit_fuel_pause called but no JitCode in cache");
    let charge_meta = &jit_code.charge_sites[charge_site_index as usize];
    let (env, stack) = rebuild_frame(
        closure,
        spilled_values,
        SpillCounts {
            num_params: charge_meta.num_params as usize,
            num_locals: charge_meta.num_locals as usize,
            num_operands: charge_meta.num_spilled as usize,
        },
    );

    let site = ParkSite {
        helper: "jit-fuel-pause",
        index: charge_site_index,
        resume_ip: charge_meta.ip,
        push_resume_value: false,
    };
    let frame = park_activation(vm, closure_val, site, env, stack);
    vm.fiber.signal = Some((crate::value::fiber::SIG_FUEL, Value::NIL));
    vm.fiber.suspended = Some(vec![frame]);

    YIELD_SENTINEL
}

/// JIT kill: the cold path of a charge whose budget is below the kill word.
///
/// Sets the fiber's signal to the terminal `:killed` error and answers `nil`,
/// which the function then returns. The caller's check after the call finds
/// the signal, as it finds a primitive's error. Nothing is spilled and no
/// frame parks: a terminal signal is never resumed, and the killed worker's
/// heap is freed whole (docs/impl/jit-fuel.md).
///
/// Parameters:
///   vm: *mut () as u64
#[no_mangle]
pub extern "C" fn elle_jit_kill(vm: u64) -> JitValue {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    vm.raise_killed();
    JitValue::nil()
}
