// audited: 2026-10-22
//! Side-exit helpers for JIT-compiled code: a yield, a park at a site, a fuel pause and a kill.
//!
//! docs/impl/jit.md
//! docs/impl/region/park.md

use super::dispatch::YIELD_SENTINEL;
use crate::jit::value::JitValue;
use crate::value::{BytecodeFrame, SuspendedFrame, Value};

// =============================================================================
// Side-Exit Helpers
// =============================================================================

/// `--trace=park`: log a JIT side-exit park with its frame shape. The resume
/// twin lives in `resume_suspended`; together they show whether a wrong value
/// was parked wrong or went wrong while parked.
fn trace_park(
    helper: &str,
    closure: &crate::value::Closure,
    site_index: u64,
    resume_ip: usize,
    env: &[Value],
    stack: &[Value],
) {
    if !crate::config::get().has_trace("park") {
        return;
    }
    eprintln!(
        "[park:{helper}] fn={} site={site_index} resume_ip={resume_ip} \
         env[{}]=[{}] stack[{}]=[{}]",
        closure.template.name().unwrap_or("<anonymous>"),
        env.len(),
        Value::type_name_line(env),
        stack.len(),
        Value::type_name_line(stack),
    );
}

/// Park-time tripwire (debug builds): every value the side-exit parks must be
/// structurally sound (`Value::malformed_reason`). A torn or zeroed slot here
/// means the compiled frame's spill diverged from the interpreter layout the
/// resume expects; detonating at the park names the function and the slot
/// instead of leaving a segfault for the resumed reader.
#[cfg(debug_assertions)]
fn check_parked_frame(
    helper: &str,
    closure: &crate::value::Closure,
    resume_ip: usize,
    env: &[Value],
    stack: &[Value],
) {
    let name = closure.template.name().unwrap_or("<anonymous>");
    for (section, values) in [("env", env), ("stack", stack)] {
        for (i, v) in values.iter().enumerate() {
            if let Some(reason) = v.malformed_reason() {
                panic!(
                    "{helper}: parked a malformed value — {reason}: \
                     fn={name} resume_ip={resume_ip} {section}[{i}] \
                     tag=0x{:x} payload=0x{:x} (env_len={} stack_len={} \
                     num_params={} num_locals={})",
                    v.tag,
                    v.payload,
                    env.len(),
                    stack.len(),
                    closure.template.num_params(),
                    closure.template.num_locals(),
                );
            }
        }
    }
}

#[cfg(not(debug_assertions))]
fn check_parked_frame(
    _helper: &str,
    _closure: &crate::value::Closure,
    _resume_ip: usize,
    _env: &[Value],
    _stack: &[Value],
) {
}

/// The spill counts a side-exit's metadata records: the buffer holds
/// `[params(num_params), locals(num_locals), operands(num_operands)]`.
struct SpillCounts {
    num_params: usize,
    num_locals: usize,
    num_operands: usize,
}

/// Rebuild the interpreter's view of a compiled frame from the closure's
/// captures and the spill buffer.
///
/// The interpreter expects:
///   env = [captures, params]         ← LoadUpvalue reads from here
///   stack = [locals, operands]       ← LoadLocal reads from here
///
/// LBox cells are first-class values in JIT registers (no auto-unwrap),
/// so spilled cells are the original objects — no re-wrapping needed.
///
/// `spilled_values` must point to `num_params + num_locals + num_operands`
/// contiguous `Value`s, or be null when that sum is 0.
fn rebuild_frame(
    closure: &crate::value::Closure,
    spilled_values: *const Value,
    counts: SpillCounts,
) -> (std::rc::Rc<Vec<Value>>, Vec<Value>) {
    let SpillCounts {
        num_params,
        num_locals,
        num_operands,
    } = counts;
    let mut env = Vec::with_capacity(closure.env.len() + num_params);
    env.extend(closure.env.iter().copied());
    for i in 0..num_params {
        env.push(unsafe { *spilled_values.add(i) });
    }
    let mut stack = Vec::with_capacity(num_locals + num_operands);
    for i in num_params..(num_params + num_locals + num_operands) {
        stack.push(unsafe { *spilled_values.add(i) });
    }
    (std::rc::Rc::new(env), stack)
}

/// Where a side-exit parks its frame, and what it names in the park trace.
struct ParkSite {
    helper: &'static str,
    index: u64,
    resume_ip: usize,
    push_resume_value: bool,
}

/// Park the executing compiled activation as an interpreter frame.
///
/// The compiled prologue pushed THIS activation's region-remap frame
/// (`elle_jit_push_region_map`), and the side-exit's pop runs after the
/// helper, so `last()` is this activation's map — captured here so
/// post-resume allocs/decrefs (re-entering via the interpreter) resolve in the
/// same frame the pre-park allocations did. A suspended callee has already
/// popped its own. What the activation owes MOVES into the frame (its slot is
/// likewise still on top), so it rides the park to the resumed body's
/// completion — the compiled twin of the interpreter park
/// (docs/impl/region/owner.md). The body's own closure is parked so a
/// self-edge resolved after resume names the right closure.
fn park_activation(
    vm: &mut crate::vm::VM,
    closure_val: Value,
    site: ParkSite,
    env: std::rc::Rc<Vec<Value>>,
    stack: Vec<Value>,
) -> SuspendedFrame {
    let closure = closure_val
        .as_closure()
        .expect("VM bug: a side-exit parks a non-closure");
    let activation_region_map = vm
        .fiber
        .activation_region_maps
        .last()
        .cloned()
        .unwrap_or_default();
    trace_park(
        site.helper,
        closure,
        site.index,
        site.resume_ip,
        &env,
        &stack,
    );
    check_parked_frame(site.helper, closure, site.resume_ip, &env, &stack);
    let code = closure.template.code();
    let activation_dues = vm.take_activation_dues();
    SuspendedFrame::Bytecode(BytecodeFrame::suspend(
        code,
        env,
        site.resume_ip,
        stack,
        site.push_resume_value,
        activation_region_map,
        activation_dues,
        closure_val,
        vm.heap(),
    ))
}

/// JIT yield side-exit: build a SuspendedFrame and set fiber.signal.
///
/// Called from JIT code when an `Emit` terminator is reached.
///
/// Parameters:
///   yielded_tag/yielded_payload: the value being yielded
///   spilled_values: *const Value (16 bytes each), or null if nothing to spill
///   yield_index: index into JitCode.yield_points
///   vm: *mut () (raw VM pointer)
///   closure_tag/closure_payload: the closure being executed (for self-tail-call detection)
///
/// Returns YIELD_SENTINEL.
///
/// # Safety
/// `spilled_values` must point to `num_spilled` contiguous `Value`s
/// (or be null when num_spilled is 0).
#[no_mangle]
pub extern "C" fn elle_jit_yield(
    yielded_tag: u64,
    yielded_payload: u64,
    spilled_values: *const Value,
    yield_index: u64,
    vm: u64, // *mut () as u64
    closure_tag: u64,
    closure_payload: u64,
    signal_bits: u64,
) -> JitValue {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let yielded = Value {
        tag: yielded_tag,
        payload: yielded_payload,
    };
    let closure_val = Value {
        tag: closure_tag,
        payload: closure_payload,
    };

    let closure = closure_val
        .as_closure()
        .expect("VM bug: elle_jit_yield called with non-closure self");

    // Look up yield point metadata from JitCode
    let bytecode_ptr = closure.template.bytecode().as_ptr();
    let jit_code = vm
        .jit_code_for(bytecode_ptr)
        .expect("VM bug: elle_jit_yield called but no JitCode in cache");
    let yield_meta = &jit_code.yield_points[yield_index as usize];
    let (env, stack) = rebuild_frame(
        closure,
        spilled_values,
        SpillCounts {
            num_params: yield_meta.num_params as usize,
            num_locals: yield_meta.num_locals as usize,
            num_operands: yield_meta.num_spilled as usize,
        },
    );
    let resume_ip = yield_meta.resume_ip;

    let sig = crate::value::fiber::SignalBits::new(signal_bits);
    // An emit of a bit the fiber withholds raises nothing: the fiber parks on
    // the denial, and the resume value answers the emit, as in `handle_emit`.
    let blocked = crate::vm::VM::emit_blocked(vm.fiber.withheld, sig);
    let denied = !blocked.is_empty();
    let raised = if denied {
        vm.park_denied_emit(sig, blocked, yielded);
        false
    } else if sig.intersects(crate::value::fiber::SIG_HALT) {
        // A halt takes neither the retain nor the record: the halted fiber is
        // promoted to `:dead` and never resumed, so its delivery has no
        // consumer (docs/impl/region/park.md).
        vm.fiber.signal = Some((sig, yielded));
        false
    } else {
        // Escape retain for the emitted value — the exact mirror of the
        // interpreter's `Emit` handler (`handle_emit`, src/vm/dispatch.rs). The
        // yielded value escapes into `fiber.signal`, where the resumer reads it
        // via `fiber/value`. The compiler emits a `DecrefRegion` at the emit's
        // decref_point (fired as this activation suspends and, on resume,
        // continues past the yield); without this incref that decref drops the
        // value's only reference while the resumer still holds it, freeing it
        // out from under the read (tests/impl/region-jit-emit-escape-uaf.lisp).
        // The symmetric release is the resume path's own pending decref,
        // `release_discarded_signal` for a fiber that never runs again, or the
        // free-path fiber discharge — all tier-agnostic (they act on
        // `fiber.signal`), so they balance this retain exactly as they balance
        // `handle_emit`'s. `region_of` no-ops an immediate.
        let heap = unsafe { &mut *vm.heap_ptr };
        let yielded_region = crate::value::arena::region_of(heap, yielded);
        crate::value::arena::incref_for_escape(
            heap,
            yielded_region,
            crate::value::arena::EscapeSite::EmitEscape,
        );
        // An ERROR emit's retain is the payload's whole delivery — record the
        // mint, exactly as `handle_emit` does, so the parked frame's discharge
        // reclaims the raise chain's own reference.
        let raised = sig.intersects(crate::value::fiber::SIG_ERROR);
        if raised {
            vm.fiber.delivery.record_mint(yielded);
        } else {
            // …and every other emit records the PARK, so a `squelch`/`attune`
            // boundary can release the delivery retain no reader will consume
            // (docs/impl/region/park.md). The record follows the retain this
            // helper took just above, which is what keeps the two balanced
            // whatever the bits are.
            vm.fiber.delivery.park_emit(sig, yielded);
        }
        vm.fiber.signal = Some((sig, yielded));
        raised
    };

    if denied || !sig.intersects(crate::value::fiber::SIG_HALT) {
        // A denial, and any signal but a halt, parks: build a frame for later
        // resumption. An error parks as a suspend does, because an error
        // unwinds nothing (docs/impl/region/unwind.md), and this frame is the
        // innermost, so it records its raise site; the emit's continuation
        // funds its own release of the restart value.
        if raised {
            vm.fiber
                .delivery
                .park_error(crate::value::fiber::RaiseSite::Emit, yielded);
        }
        let site = ParkSite {
            helper: "jit-yield",
            index: yield_index,
            resume_ip,
            push_resume_value: true,
        };
        let frame = park_activation(vm, closure_val, site, env, stack);
        vm.fiber.suspended = Some(vec![frame]);
    }

    YIELD_SENTINEL
}

/// JIT park at a site: append this frame to fiber.suspended.
///
/// Called from JIT code when the check after a call or an instruction that
/// may raise finds a pending signal. Builds this frame's SuspendedFrame and
/// appends it to the chain the callee parked, an error included. A halt
/// builds no frame. Where nothing parked yet, this frame is the innermost of
/// an error's chain, and it records the raise site in the delivery ledger:
/// `Call` where a restart delivers into the site's result, `NoResult` where
/// the instruction produces none (docs/impl/vm.md).
///
/// Parameters:
///   spilled_values: *const Value (16 bytes each)
///   call_site_index: index into JitCode.call_sites
///   vm: *mut () as u64
///   closure_tag/closure_payload: the closure being executed
///
/// Returns YIELD_SENTINEL.
///
/// # Safety
/// `spilled_values` must point to `num_spilled` contiguous `Value`s
/// (or be null when num_spilled is 0).
#[no_mangle]
pub extern "C" fn elle_jit_yield_through_call(
    spilled_values: *const Value,
    call_site_index: u64,
    vm: u64, // *mut () as u64
    closure_tag: u64,
    closure_payload: u64,
) -> JitValue {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let (sig, payload) = vm
        .fiber
        .signal
        .expect("VM bug: elle_jit_yield_through_call reached with no pending signal");
    if sig.intersects(crate::value::fiber::SIG_HALT) {
        return YIELD_SENTINEL;
    }
    let closure_val = Value {
        tag: closure_tag,
        payload: closure_payload,
    };

    let closure = closure_val
        .as_closure()
        .expect("VM bug: elle_jit_yield_through_call called with non-closure");

    // Look up call site metadata from JitCode
    let bytecode_ptr = closure.template.bytecode().as_ptr();
    let jit_code = vm
        .jit_code_for(bytecode_ptr)
        .expect("VM bug: elle_jit_yield_through_call called but no JitCode in cache");
    let call_meta = &jit_code.call_sites[call_site_index as usize];
    let (env, stack) = rebuild_frame(
        closure,
        spilled_values,
        SpillCounts {
            num_params: call_meta.num_params as usize,
            num_locals: call_meta.num_locals as usize,
            num_operands: call_meta.num_spilled as usize,
        },
    );
    // On resume, the callee's return value flows as current_value and is
    // pushed as the site's result, unless the site's instruction produces none.
    let site = ParkSite {
        helper: "jit-yield-through-call",
        index: call_site_index,
        resume_ip: call_meta.resume_ip,
        push_resume_value: call_meta.push_resume_value,
    };

    // The innermost frame of an error's chain records its raise site.
    if sig.intersects(crate::value::fiber::SIG_ERROR) && vm.fiber.suspended.is_none() {
        let raise_site = if site.push_resume_value {
            crate::value::fiber::RaiseSite::Call
        } else {
            crate::value::fiber::RaiseSite::NoResult
        };
        vm.fiber.delivery.park_error(raise_site, payload);
    }
    let caller_frame = park_activation(vm, closure_val, site, env, stack);

    // Append caller frame to the existing suspended chain.
    let mut frames = vm.fiber.suspended.take().unwrap_or_default();
    frames.push(caller_frame);
    vm.fiber.suspended = Some(frames);

    YIELD_SENTINEL
}

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
/// heap is freed whole (docs/impl/jit.md).
///
/// Parameters:
///   vm: *mut () as u64
#[no_mangle]
pub extern "C" fn elle_jit_kill(vm: u64) -> JitValue {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    vm.raise_killed();
    JitValue::nil()
}

/// Check if any non-OK signal is pending on the VM.
/// Returns TRUE if set, FALSE otherwise.
///
/// The one check after a call or an instruction that may raise: an error, a
/// halt, and every suspending signal (SIG_YIELD, SIG_SWITCH, user-defined).
///
/// Checks `!is_empty()` rather than matching specific signal bits, because
/// I/O primitives return compound signals like `SIG_YIELD | SIG_IO` and
/// SIG_SWITCH must also be detected for fiber/resume trampolining.
#[no_mangle]
pub extern "C" fn elle_jit_has_signal(vm: u64) -> JitValue {
    let vm = unsafe { &*(vm as *const crate::vm::VM) };
    JitValue::bool_val(vm.fiber.signal.as_ref().is_some_and(|(b, _)| !b.is_empty()))
}

#[cfg(test)]
mod tests;
